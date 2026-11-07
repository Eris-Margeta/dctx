// SPDX-License-Identifier: Apache-2.0
//! Sans-IO frame codec: u32 little-endian length prefix followed by exactly that many body bytes.
//! Implements plugin-api wire.md §Bytes and requirements W-FRAME: zero/oversized lengths refused before
//! allocation, split prefixes/bodies reassembled, truncation at EOF refused, 64 KiB bootstrap cap then the
//! negotiated cap (at most 1 MiB, lowered never raised). Depends on refusal; used by stream and callers.

use crate::records::failure::ErrorCode;
use crate::refusal::{Predicate, Refusal, Stage};

/// Frame cap before negotiation.
pub const BOOTSTRAP_FRAME_CAP: u32 = 65_536;
/// Absolute frame ceiling.
pub const MAX_FRAME_CAP: u32 = 1_048_576;

fn too_large() -> Refusal {
    Refusal::new(
        ErrorCode::ResourceLimit,
        Stage::Frame,
        Predicate::WFrame,
        "frame-too-large",
    )
}

/// Encodes one frame, refusing an empty body or one above `cap`.
/// # Errors
/// Refuses empty bodies and lengths above the supplied cap or protocol maximum.
pub fn encode_frame(body: &[u8], cap: u32) -> Result<Vec<u8>, Refusal> {
    if body.is_empty() {
        return Err(Refusal::frame("zero-length"));
    }
    let length = u32::try_from(body.len()).map_err(|_| too_large())?;
    if length > cap.min(MAX_FRAME_CAP) {
        return Err(too_large());
    }
    let mut frame = Vec::with_capacity(body.len() + 4);
    frame.extend_from_slice(&length.to_le_bytes());
    frame.extend_from_slice(body);
    Ok(frame)
}

/// Incremental frame decoder holding at most one partial frame.
#[derive(Debug)]
pub struct FrameDecoder {
    cap: u32,
    prefix: [u8; 4],
    prefix_len: usize,
    expected: usize,
    body: Vec<u8>,
    poisoned: bool,
}

impl Default for FrameDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameDecoder {
    /// A decoder at the bootstrap cap.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn new() -> Self {
        Self {
            cap: BOOTSTRAP_FRAME_CAP,
            prefix: [0; 4],
            prefix_len: 0,
            expected: 0,
            body: Vec::new(),
            poisoned: false,
        }
    }

    /// Current cap.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn cap(&self) -> u32 {
        self.cap
    }

    /// Applies a negotiated cap; it may be lowered or set once from bootstrap, never above 1 MiB.
    /// # Errors
    /// Refuses zero or a cap above `MAX_FRAME_CAP`.
    pub fn set_cap(&mut self, cap: u32) -> Result<(), Refusal> {
        if cap == 0 || cap > MAX_FRAME_CAP {
            return Err(too_large());
        }
        self.cap = cap;
        Ok(())
    }

    /// Whether no partial frame is buffered.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn is_idle(&self) -> bool {
        self.prefix_len == 0 && self.body.is_empty()
    }

    /// Consumes bytes from `input`, stopping after at most one completed frame.
    /// Returns the number of bytes consumed and the completed body, if any.
    /// After a refusal the stream is desynchronized and every later call refuses.
    /// # Errors
    /// Refuses malformed or oversized frames and any input after a framing refusal desynchronized the stream.
    pub fn push(&mut self, input: &[u8]) -> Result<(usize, Option<Vec<u8>>), Refusal> {
        if self.poisoned {
            return Err(Refusal::frame("stream-desynchronized"));
        }
        let result = self.push_inner(input);
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }

    fn push_inner(&mut self, input: &[u8]) -> Result<(usize, Option<Vec<u8>>), Refusal> {
        let mut used = 0;
        if self.prefix_len < 4 {
            let take = (4 - self.prefix_len).min(input.len());
            self.prefix[self.prefix_len..self.prefix_len + take].copy_from_slice(&input[..take]);
            self.prefix_len += take;
            used += take;
            if self.prefix_len < 4 {
                return Ok((used, None));
            }
            let length = u32::from_le_bytes(self.prefix);
            if length == 0 {
                return Err(Refusal::frame("zero-length"));
            }
            if length > self.cap {
                return Err(too_large());
            }
            self.expected = usize::try_from(length).map_err(|_| too_large())?;
            self.body = Vec::with_capacity(self.expected);
        }
        let take = (self.expected - self.body.len()).min(input.len() - used);
        self.body.extend_from_slice(&input[used..used + take]);
        used += take;
        if self.body.len() < self.expected {
            return Ok((used, None));
        }
        self.prefix_len = 0;
        self.expected = 0;
        Ok((used, Some(std::mem::take(&mut self.body))))
    }

    /// Declares end of stream: a buffered partial prefix or body is refused.
    /// # Errors
    /// Refuses a truncated prefix/body or an already desynchronized stream.
    pub fn finish(&self) -> Result<(), Refusal> {
        if self.poisoned {
            Err(Refusal::frame("stream-desynchronized"))
        } else if self.prefix_len > 0 && self.prefix_len < 4 {
            Err(Refusal::frame("truncated-prefix"))
        } else if self.prefix_len == 4 {
            Err(Refusal::frame("truncated-body"))
        } else {
            Ok(())
        }
    }

    /// Feeds a whole buffer and collects every completed body.
    /// # Errors
    /// Propagates framing refusals from `Self::push`, including an already desynchronized stream.
    pub fn push_all(&mut self, mut input: &[u8]) -> Result<Vec<Vec<u8>>, Refusal> {
        let mut bodies = Vec::new();
        while !input.is_empty() {
            let (used, body) = self.push(input)?;
            bodies.extend(body);
            input = &input[used..];
        }
        Ok(bodies)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reassembles_split_prefix_and_body() {
        let frame = encode_frame(b"{\"a\":1}", BOOTSTRAP_FRAME_CAP).expect("frame");
        for split in 0..=frame.len() {
            let mut decoder = FrameDecoder::new();
            let mut bodies = decoder.push_all(&frame[..split]).expect("first");
            bodies.extend(decoder.push_all(&frame[split..]).expect("second"));
            assert_eq!(bodies, vec![b"{\"a\":1}".to_vec()]);
            assert!(decoder.is_idle());
            assert_eq!(decoder.finish(), Ok(()));
        }
    }

    #[test]
    fn back_to_back_frames_stop_after_each_frame() {
        let mut stream = encode_frame(b"{}", 16).expect("a");
        stream.extend(encode_frame(b"{\"b\":2}", 16).expect("b"));
        let mut decoder = FrameDecoder::new();
        let (used, body) = decoder.push(&stream).expect("push");
        assert_eq!((used, body), (6, Some(b"{}".to_vec())));
        assert_eq!(
            decoder.push_all(&stream[used..]).expect("rest"),
            vec![b"{\"b\":2}".to_vec()]
        );
    }

    #[test]
    fn refuses_zero_oversize_and_truncation() {
        let mut decoder = FrameDecoder::new();
        assert_eq!(
            decoder.push(&[0, 0, 0, 0]).unwrap_err().reason,
            "zero-length"
        );
        assert_eq!(
            decoder.push(b"{}").unwrap_err().reason,
            "stream-desynchronized"
        );
        let mut decoder = FrameDecoder::new();
        let refusal = decoder.push(&65_537u32.to_le_bytes()).unwrap_err();
        assert_eq!(refusal.code, ErrorCode::ResourceLimit);
        let mut decoder = FrameDecoder::new();
        assert!(decoder.push(&65_536u32.to_le_bytes()).is_ok());
        let mut decoder = FrameDecoder::new();
        decoder.push(&[1, 0, 0]).expect("partial");
        assert_eq!(decoder.finish().unwrap_err().reason, "truncated-prefix");
        decoder.push(&[0, b'{']).expect("rest");
        assert_eq!(decoder.finish(), Ok(()));
        let mut decoder = FrameDecoder::new();
        decoder.push(&[2, 0, 0, 0, b'{']).expect("partial");
        assert_eq!(decoder.finish().unwrap_err().reason, "truncated-body");
    }

    #[test]
    fn caps_follow_negotiation() {
        let mut decoder = FrameDecoder::new();
        assert_eq!(decoder.cap(), BOOTSTRAP_FRAME_CAP);
        decoder.set_cap(MAX_FRAME_CAP).expect("negotiated");
        assert!(decoder.push(&MAX_FRAME_CAP.to_le_bytes()).is_ok());
        assert!(decoder.set_cap(MAX_FRAME_CAP + 1).is_err());
        assert!(decoder.set_cap(0).is_err());
        assert!(encode_frame(b"", 16).is_err());
        assert_eq!(
            encode_frame(&[b' '; 17], 16).unwrap_err().code,
            ErrorCode::ResourceLimit
        );
        assert!(encode_frame(&vec![b' '; 2_000_000], u32::MAX).is_err());
        assert_eq!(FrameDecoder::default().cap(), BOOTSTRAP_FRAME_CAP);
    }
}

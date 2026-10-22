//! Blocking `std::io` adapter over the sans-IO frame codec (the crate's only IO; records stay IO-free).
//! Implements plugin-api wire.md §Bytes "split reads/writes are legal and must reconstruct the same message"
//! for any reader/writer that returns short counts or `Interrupted`. Depends on frame and refusal.
//! Used by host process adapters and worker binaries; stderr is never read here.

use std::io::{ErrorKind, Read, Write};

use crate::frame::{FrameDecoder, encode_frame};
use crate::refusal::Refusal;

const READ_CHUNK: usize = 8192;

/// A stream failure: an IO error kind or a protocol refusal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamError {
    /// The underlying reader or writer failed.
    Io(ErrorKind),
    /// The bytes violated the frame protocol.
    Protocol(Refusal),
}

impl From<Refusal> for StreamError {
    fn from(refusal: Refusal) -> Self {
        Self::Protocol(refusal)
    }
}

/// Reads frames from a byte stream, keeping bytes that belong to the next frame.
#[derive(Debug)]
pub struct FrameReader<R> {
    reader: R,
    decoder: FrameDecoder,
    pending: Vec<u8>,
    start: usize,
}

impl<R: Read> FrameReader<R> {
    /// Wraps `reader` with a bootstrap-cap decoder.
    pub fn new(reader: R) -> Self {
        Self { reader, decoder: FrameDecoder::new(), pending: Vec::new(), start: 0 }
    }

    /// The frame decoder, for applying a negotiated cap.
    pub fn decoder_mut(&mut self) -> &mut FrameDecoder {
        &mut self.decoder
    }

    /// Next frame body; `Ok(None)` at a clean end of stream between frames.
    /// # Errors
    /// Returns an I/O error on a failed read, or a framing refusal for malformed, oversized, or truncated input.
    pub fn next_frame(&mut self) -> Result<Option<Vec<u8>>, StreamError> {
        loop {
            if self.start < self.pending.len() {
                let (used, body) = self.decoder.push(&self.pending[self.start..])?;
                self.start += used;
                if let Some(body) = body {
                    return Ok(Some(body));
                }
                continue;
            }
            self.pending.clear();
            self.start = 0;
            let mut chunk = [0u8; READ_CHUNK];
            match self.reader.read(&mut chunk) {
                Ok(0) => {
                    self.decoder.finish()?;
                    return Ok(None);
                }
                Ok(count) => self.pending.extend_from_slice(&chunk[..count]),
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => return Err(StreamError::Io(error.kind())),
            }
        }
    }
}

/// Writes one frame completely, tolerating short writes and interruptions, then flushes.
/// # Errors
/// Returns a framing refusal for an invalid body/cap, or an I/O error when writing or flushing fails.
pub fn write_frame<W: Write>(writer: &mut W, body: &[u8], cap: u32) -> Result<(), StreamError> {
    let frame = encode_frame(body, cap)?;
    writer.write_all(&frame).map_err(|error| StreamError::Io(error.kind()))?;
    writer.flush().map_err(|error| StreamError::Io(error.kind()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    struct Trickle {
        data: Vec<u8>,
        pos: usize,
        step: usize,
        interrupt: bool,
    }

    impl Read for Trickle {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.interrupt = !self.interrupt;
            if self.interrupt {
                return Err(io::Error::from(ErrorKind::Interrupted));
            }
            let count = self.step.min(buf.len()).min(self.data.len() - self.pos);
            buf[..count].copy_from_slice(&self.data[self.pos..self.pos + count]);
            self.pos += count;
            Ok(count)
        }
    }

    struct ShortWriter {
        out: Vec<u8>,
        calls: usize,
    }

    impl Write for ShortWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.calls += 1;
            if self.calls == 3 {
                self.calls = 0;
                return Err(io::Error::from(ErrorKind::Interrupted));
            }
            let count = buf.len().min(2);
            self.out.extend_from_slice(&buf[..count]);
            Ok(count)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn short_writes_and_trickled_reads_reconstruct_frames() {
        let mut writer = ShortWriter { out: Vec::new(), calls: 0 };
        write_frame(&mut writer, b"{\"a\":1}", 64).expect("write");
        write_frame(&mut writer, b"{}", 64).expect("write");
        for step in 1..=12 {
            let data = writer.out.clone();
            let mut reader = FrameReader::new(Trickle { data, pos: 0, step, interrupt: false });
            assert_eq!(reader.next_frame(), Ok(Some(b"{\"a\":1}".to_vec())));
            assert_eq!(reader.next_frame(), Ok(Some(b"{}".to_vec())));
            assert_eq!(reader.next_frame(), Ok(None));
        }
    }

    #[test]
    fn truncated_stream_and_io_errors_are_typed() {
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::from(ErrorKind::BrokenPipe))
            }
        }
        let mut reader = FrameReader::new(&[3u8, 0, 0, 0, b'{'][..]);
        assert!(matches!(reader.next_frame(), Err(StreamError::Protocol(r)) if r.reason == "truncated-body"));
        assert_eq!(FrameReader::new(Broken).next_frame(), Err(StreamError::Io(ErrorKind::BrokenPipe)));
        let mut reader = FrameReader::new(&[0u8; 4][..]);
        reader.decoder_mut().set_cap(16).expect("cap");
        assert!(matches!(reader.next_frame(), Err(StreamError::Protocol(_))));
        assert!(write_frame(&mut Vec::new(), b"", 16).is_err());
    }
}

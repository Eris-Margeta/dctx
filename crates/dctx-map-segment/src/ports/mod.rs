//! Positioned reads that fill a bounded buffer before decode.
//! Implements SEGMENTS-v1: refuse a file larger than the ceiling before allocating it.
//! Depends on [`crate::domain::SegmentError`] and [`crate::DecoderLimits`]. Adapters implement [`SegmentRead`].
//! A short read is [`SegmentError::Truncated`]. The trait performs no decoding.

use crate::{DecoderLimits, SegmentError};

/// A source of segment bytes. `read_at` must fill `buf` or return an error.
pub trait SegmentRead {
    /// Byte length of the source.
    fn len(&self) -> u64;

    /// Whether [`Self::len`] is zero.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Copies `buf.len()` bytes starting at `offset` into `buf`.
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), SegmentError>;
}

/// Reads a whole segment when `reader.len()` is within `limits`.
pub fn load_bounded<R: SegmentRead + ?Sized>(
    reader: &R,
    limits: &DecoderLimits,
) -> Result<Vec<u8>, SegmentError> {
    let limits = limits.effective();
    let len = reader.len();
    if len > limits.max_segment_bytes {
        return Err(SegmentError::Limit("segment"));
    }
    let size = usize::try_from(len).map_err(|_| SegmentError::OffsetOverflow)?;
    let mut buf = vec![0u8; size];
    if size > 0 {
        reader.read_at(0, &mut buf)?;
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Good(&'static [u8]);

    impl SegmentRead for Good {
        fn len(&self) -> u64 {
            u64::try_from(self.0.len()).unwrap_or(u64::MAX)
        }

        fn is_empty(&self) -> bool {
            self.0.is_empty()
        }

        fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), SegmentError> {
            let start = usize::try_from(offset).map_err(|_| SegmentError::OffsetOverflow)?;
            let end = start.checked_add(buf.len()).ok_or(SegmentError::OffsetOverflow)?;
            let src = self.0.get(start..end).ok_or(SegmentError::Truncated {
                at: offset,
                need: offset.saturating_add(u64::try_from(buf.len()).unwrap_or(u64::MAX)),
            })?;
            buf.copy_from_slice(src);
            Ok(())
        }
    }

    struct Short;

    impl SegmentRead for Short {
        fn len(&self) -> u64 {
            4
        }

        fn is_empty(&self) -> bool {
            false
        }

        fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), SegmentError> {
            let _ = buf;
            Err(SegmentError::Truncated {
                at: offset,
                need: offset.saturating_add(4),
            })
        }
    }

    struct Boom;

    impl SegmentRead for Boom {
        fn len(&self) -> u64 {
            DecoderLimits::standard().max_segment_bytes.saturating_add(1)
        }

        fn is_empty(&self) -> bool {
            false
        }

        fn read_at(&self, _offset: u64, _buf: &mut [u8]) -> Result<(), SegmentError> {
            panic!("read past the cap");
        }
    }

    #[test]
    fn oversize_inputs_are_refused_before_the_read() {
        let good = load_bounded(&Good(b"abcd"), &DecoderLimits::standard()).unwrap();
        assert_eq!(good, b"abcd");
        assert_eq!(
            load_bounded(&Short, &DecoderLimits::standard()).unwrap_err(),
            SegmentError::Truncated { at: 0, need: 4 }
        );
        assert_eq!(
            load_bounded(&Boom, &DecoderLimits::standard()).unwrap_err(),
            SegmentError::Limit("segment")
        );
    }
}

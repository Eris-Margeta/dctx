//! Result checks that need the pending request: chunk arithmetic, probe echoes and hook output rules.
//! Implements wire.md §Bytes "offset + `decoded_length` <= size; eof iff equal; never more than requested"
//! (W-BYTES), the exact probe challenge echo and S-CONFIG/S-RESULT "ordinary hooks return `command_output`=[]".
//! Depends on records; used by `session::Session` when a response resolves a pending request.

use crate::records::broker::ChunkRead;
use crate::records::requests::Request;
use crate::records::results::{ChunkResult, ResultBody};
use crate::refusal::{Predicate, Refusal};

fn bytes(reason: &'static str) -> Refusal {
    Refusal::semantic(Predicate::WBytes, reason)
}

/// Checks a delivered chunk against the read that requested it (checked arithmetic throughout).
/// # Errors
/// Refuses mismatched offsets, excessive lengths, overflow, inconsistent EOF/size, or invalid read completion.
pub fn check_chunk(read: &ChunkRead, chunk: &ChunkResult) -> Result<(), Refusal> {
    let delivered = u64::try_from(chunk.bytes.len()).map_err(|_| bytes("chunk-length"))?;
    if chunk.offset != read.offset {
        return Err(bytes("offset-mismatch"));
    }
    if delivered > read.length {
        return Err(bytes("more-than-requested"));
    }
    let end = chunk.offset.checked_add(delivered).ok_or(bytes("offset-overflow"))?;
    if end > chunk.size {
        return Err(bytes("beyond-size"));
    }
    if chunk.eof != (end == chunk.size) {
        return Err(bytes("eof-mismatch"));
    }
    Ok(())
}

/// Applies every request-dependent rule to a resolved result.
/// # Errors
/// Refuses a result inconsistent with its request, including chunk bounds, probe echoes, and hook output rules.
pub fn check_result(request: &Request, body: &ResultBody) -> Result<(), Refusal> {
    match (request, body) {
        (Request::OutputRead(read) | Request::FileRead(read) | Request::StateRead(read), ResultBody::Chunk(chunk)) => {
            check_chunk(read, chunk)
        }
        (Request::Probe(probe), ResultBody::Probe { invocation, challenge, .. }) => {
            if invocation == &probe.invocation && challenge == &probe.challenge {
                Ok(())
            } else {
                Err(Refusal::session(Predicate::WBinding, "probe-echo-mismatch"))
            }
        }
        (Request::Invoke(invocation), ResultBody::Invoke { command_output, .. }) => {
            if invocation.hook().is_some() && !command_output.is_empty() {
                Err(Refusal::semantic(Predicate::SResult, "hook-command-output"))
            } else {
                Ok(())
            }
        }
        (Request::FileStage(stage), ResultBody::FileStage { next_offset, complete, .. }) => {
            let delivered = u64::try_from(stage.bytes.len()).map_err(|_| bytes("chunk-length"))?;
            let end = stage.offset.checked_add(delivered).ok_or(bytes("offset-overflow"))?;
            if *next_offset != end || *complete != stage.final_chunk {
                Err(bytes("upload-progress-mismatch"))
            } else {
                Ok(())
            }
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::records::bytes::Chunk;
    use crate::records::scalars::Token;

    fn read(offset: u64, length: u64) -> ChunkRead {
        ChunkRead { handle: Token::new("file:ignore").expect("token"), offset, length, precondition: None }
    }

    fn chunk(bytes: &[u8], offset: u64, size: u64, eof: bool) -> ChunkResult {
        ChunkResult {
            bytes: Chunk::new(bytes).expect("chunk"),
            offset,
            size,
            eof,
            precondition: None,
            receipt: Token::new("receipt:read").expect("token"),
        }
    }

    #[test]
    fn chunk_arithmetic_with_positive_controls() {
        assert_eq!(check_chunk(&read(0, 3), &chunk(b"abc", 0, 3, true)), Ok(()));
        assert_eq!(check_chunk(&read(0, 3), &chunk(b"ab", 0, 3, false)), Ok(()));
        assert_eq!(check_chunk(&read(3, 3), &chunk(b"", 3, 3, true)), Ok(()));
        assert_eq!(check_chunk(&read(0, 3), &chunk(b"abc", 0, 4, true)).unwrap_err().reason, "eof-mismatch");
        assert_eq!(check_chunk(&read(0, 3), &chunk(b"abc", 0, 3, false)).unwrap_err().reason, "eof-mismatch");
        assert_eq!(check_chunk(&read(0, 2), &chunk(b"abc", 0, 3, true)).unwrap_err().reason, "more-than-requested");
        assert_eq!(check_chunk(&read(1, 3), &chunk(b"abc", 1, 3, true)).unwrap_err().reason, "beyond-size");
        assert_eq!(check_chunk(&read(0, 3), &chunk(b"abc", 1, 4, true)).unwrap_err().reason, "offset-mismatch");
        let huge = read(u64::MAX, 3);
        assert_eq!(check_chunk(&huge, &chunk(b"abc", u64::MAX, 3, true)).unwrap_err().reason, "offset-overflow");
    }
}

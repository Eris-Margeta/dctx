// SPDX-License-Identifier: Apache-2.0
//! Bounded chunked transfers through the broker: whole-file `file.read` and `file.stage` uploads.
//! Implements PLUGINS.md §3.3 "later chunks must bind that token" and §6.2 step 6 "Upload the replacement
//! through bounded file.stage chunks"; chunk size fits the negotiated frame after base64 (wire.md §Bytes).
//! Depends on `ports::Broker`; used by `app::protect`.

use dctx_plugin_protocol::records::broker::{ChunkRead, FileStage};
use dctx_plugin_protocol::records::bytes::{Chunk, MAX_CHUNK_BYTES};
use dctx_plugin_protocol::records::common::Limits;
use dctx_plugin_protocol::records::events::ProgressUnit;
use dctx_plugin_protocol::records::scalars::Token;
use dctx_plugin_protocol::{ErrorCode, Phase};

use crate::app::failure::Failure;
use crate::ports::{Broker, BrokerFailure};

/// Envelope allowance reserved in each frame besides the base64 payload.
const ENVELOPE_BYTES: u64 = 1024;

/// A whole file as delivered under one precondition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Delivered {
    /// File bytes.
    pub bytes: Vec<u8>,
    /// The host precondition every chunk was bound to.
    pub precondition: Token,
}

/// Largest raw chunk that fits the invocation's chunk and frame limits once base64-encoded.
/// # Errors
/// Refuses a negotiated frame cap too small to carry a chunk.
pub fn chunk_size(limits: &Limits) -> Result<u64, Failure> {
    let frame_fit = limits.frame_bytes.saturating_sub(ENVELOPE_BYTES) / 4 * 3;
    let size = limits
        .chunk_bytes
        .min(frame_fit)
        .min(MAX_CHUNK_BYTES as u64);
    if size == 0 {
        Err(Failure::limit("frame-too-small", "frame_bytes"))
    } else {
        Ok(size)
    }
}

fn protocol(reason: &'static str) -> Failure {
    Failure::from_broker(BrokerFailure::Protocol(reason), false, &[])
}

/// Reads a whole file: refuses before more delivery when its size exceeds `max_bytes`.
/// `phase` names the progress phase (one file per phase), or `None` for no progress samples.
/// # Errors
/// Refuses broker failures, changed/missing preconditions, malformed chunks, oversized files, or exceeded read budgets.
pub fn read_file(
    broker: &mut dyn Broker,
    resource: &Token,
    max_bytes: u64,
    limits: &Limits,
    phase: Option<&'static str>,
) -> Result<Delivered, Failure> {
    let chunk = chunk_size(limits)?;
    let mut bytes = Vec::new();
    let mut precondition: Option<Token> = None;
    loop {
        let offset = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        let read = ChunkRead {
            handle: resource.clone(),
            offset,
            length: chunk,
            precondition: precondition.clone(),
        };
        let result = broker
            .file_read(&read)
            .map_err(|failure| Failure::from_broker(failure, false, &[]))?;
        let token = result
            .precondition
            .clone()
            .ok_or_else(|| protocol("missing-precondition"))?;
        match &precondition {
            Some(bound) if *bound != token => {
                return Err(Failure::refused(
                    ErrorCode::Conflict,
                    Phase::Broker,
                    "precondition-changed",
                ));
            }
            _ => precondition = Some(token),
        }
        if result.size > max_bytes {
            return Err(Failure::limit("ignore-oversized", "max_bytes"));
        }
        let delivered = u64::try_from(result.bytes.len()).unwrap_or(u64::MAX);
        if result.offset != offset
            || delivered > chunk
            || offset.saturating_add(delivered) > result.size
        {
            return Err(protocol("chunk-out-of-bounds"));
        }
        bytes.extend_from_slice(result.bytes.as_bytes());
        let total = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if total > limits.read_bytes {
            return Err(Failure::limit("read-budget", "read_bytes"));
        }
        if let Some(phase) = phase {
            broker.progress(phase, total, Some(result.size), ProgressUnit::Bytes);
        }
        if result.eof {
            break;
        }
        if delivered == 0 {
            return Err(protocol("empty-chunk"));
        }
    }
    let precondition = precondition.ok_or_else(|| protocol("missing-precondition"))?;
    Ok(Delivered {
        bytes,
        precondition,
    })
}

/// Uploads `bytes` in sealed chunks bound to `precondition`; returns the sealed upload ID.
/// # Errors
/// Refuses broker failures, exceeded upload/chunk budgets, or inconsistent upload progress.
pub fn upload(
    broker: &mut dyn Broker,
    resource: &Token,
    precondition: &Token,
    bytes: &[u8],
    limits: &Limits,
) -> Result<Token, Failure> {
    let chunk = usize::try_from(chunk_size(limits)?).unwrap_or(MAX_CHUNK_BYTES);
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limits.upload_bytes {
        return Err(Failure::limit("upload-budget", "upload_bytes"));
    }
    let total = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    let mut upload: Option<Token> = None;
    let mut offset = 0usize;
    loop {
        let end = bytes.len().min(offset + chunk);
        let final_chunk = end == bytes.len();
        let stage = FileStage {
            resource: resource.clone(),
            precondition: precondition.clone(),
            upload: upload.clone(),
            offset: u64::try_from(offset).unwrap_or(u64::MAX),
            bytes: Chunk::new(&bytes[offset..end])
                .map_err(|_| Failure::limit("chunk-too-large", "chunk_bytes"))?,
            final_chunk,
        };
        let staged = broker
            .file_stage(&stage)
            .map_err(|failure| Failure::from_broker(failure, false, &[]))?;
        if upload.as_ref().is_some_and(|bound| *bound != staged.upload)
            || staged.next_offset != u64::try_from(end).unwrap_or(u64::MAX)
            || staged.complete != final_chunk
        {
            return Err(protocol("upload-progress-mismatch"));
        }
        upload = Some(staged.upload);
        broker.progress(
            "stage-upload",
            staged.next_offset,
            Some(total),
            ProgressUnit::Bytes,
        );
        if final_chunk {
            break;
        }
        offset = end;
    }
    upload.ok_or_else(|| protocol("missing-upload"))
}

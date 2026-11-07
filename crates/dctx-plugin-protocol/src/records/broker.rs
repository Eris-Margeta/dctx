// SPDX-License-Identifier: Apache-2.0
//! Plugin broker request params: output, file, program and state effects.
//! Implements requests.schema.json `output.*`, `file.*`, `program.run` and `state.*`; Git has exactly one
//! recipe with handle arguments, never argv (S-CAPABILITY). Depends on json, scalars, bytes and refusal.
//! Used by `requests::Request`; replay keys use `action_id` (S-REPLAY).

use crate::json::fields::{Fields, Obj, array, boolean, constant, int_in, uint, unique_list};
use crate::json::value::Json;
use crate::records::bytes::Chunk;
use crate::records::scalars::Token;
use crate::refusal::Refusal;

/// A bounded read of a handle: `output.read`, `file.read` or `state.read`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChunkRead {
    /// Output, file or state handle.
    pub handle: Token,
    /// Start offset.
    pub offset: u64,
    /// Requested length, 1 through 65536.
    pub length: u64,
    /// Precondition token from the first chunk (absent on `output.read`).
    pub precondition: Option<Token>,
}

impl ChunkRead {
    pub(crate) fn decode(
        value: &Json,
        handle_key: &str,
        allow_precondition: bool,
    ) -> Result<Self, Refusal> {
        let mut f = Fields::of(value)?;
        let read = Self {
            handle: Token::decode(f.req(handle_key)?)?,
            offset: uint(f.req("offset")?)?,
            length: int_in(f.req("length")?, 1, 65_536)?,
            precondition: if allow_precondition {
                f.opt("precondition").map(Token::decode).transpose()?
            } else {
                None
            },
        };
        f.finish()?;
        Ok(read)
    }

    pub(crate) fn encode(&self, handle_key: &str) -> Json {
        Obj::new()
            .put(handle_key, self.handle.encode())
            .put("offset", Json::uint(self.offset))
            .put("length", Json::uint(self.length))
            .maybe(
                "precondition",
                self.precondition.as_ref().map(Token::encode),
            )
            .build()
    }
}

/// `output.list`: page with a host-bound cursor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputList {
    /// Active invocation.
    pub invocation: Token,
    /// Host-issued cursor.
    pub cursor: Token,
    /// Page size, 1 through 64.
    pub max_items: u64,
}

/// `file.stage`: one contiguous upload chunk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileStage {
    /// File handle.
    pub resource: Token,
    /// Resource precondition.
    pub precondition: Token,
    /// Upload ID; absent only on the first chunk at offset zero.
    pub upload: Option<Token>,
    /// Chunk offset.
    pub offset: u64,
    /// Chunk bytes.
    pub bytes: Chunk,
    /// Whether this chunk seals the upload.
    pub final_chunk: bool,
}

/// `file.replace`: atomic replace from a sealed upload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileReplace {
    /// File handle.
    pub resource: Token,
    /// Resource precondition.
    pub precondition: Token,
    /// Sealed upload.
    pub upload: Token,
    /// Replay key.
    pub action_id: Token,
}

/// `program.run` with the sole v1 recipe `git.add-ignore`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitAddIgnore {
    /// Ignore-file handle.
    pub resource: Token,
    /// Resource precondition.
    pub precondition: Token,
    /// Repository administrative handle.
    pub repository: Token,
    /// Replay key.
    pub action_id: Token,
}

/// `state.write`: replace one bounded state object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateWrite {
    /// State handle.
    pub handle: Token,
    /// State precondition.
    pub precondition: Token,
    /// New bytes.
    pub bytes: Chunk,
    /// Provenance handles (at most 128, unique).
    pub provenance: Vec<Token>,
    /// Replay key.
    pub action_id: Token,
}

/// `state.reclaim`: reclaim an own unpinned state object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateReclaim {
    /// State handle.
    pub handle: Token,
    /// State precondition.
    pub precondition: Token,
    /// Replay key.
    pub action_id: Token,
}

impl OutputList {
    pub(crate) fn decode(value: &Json) -> Result<Self, Refusal> {
        let mut f = Fields::of(value)?;
        let list = Self {
            invocation: Token::decode(f.req("invocation")?)?,
            cursor: Token::decode(f.req("cursor")?)?,
            max_items: int_in(f.req("max_items")?, 1, 64)?,
        };
        f.finish()?;
        Ok(list)
    }

    pub(crate) fn encode(&self) -> Json {
        Obj::new()
            .put("invocation", self.invocation.encode())
            .put("cursor", self.cursor.encode())
            .put("max_items", Json::uint(self.max_items))
            .build()
    }
}

impl FileStage {
    pub(crate) fn decode(value: &Json) -> Result<Self, Refusal> {
        let mut f = Fields::of(value)?;
        let stage = Self {
            resource: Token::decode(f.req("resource")?)?,
            precondition: Token::decode(f.req("precondition")?)?,
            upload: f.opt("upload").map(Token::decode).transpose()?,
            offset: uint(f.req("offset")?)?,
            bytes: Chunk::decode(f.req("bytes")?)?,
            final_chunk: boolean(f.req("final")?)?,
        };
        f.finish()?;
        Ok(stage)
    }

    pub(crate) fn encode(&self) -> Json {
        Obj::new()
            .put("resource", self.resource.encode())
            .put("precondition", self.precondition.encode())
            .maybe("upload", self.upload.as_ref().map(Token::encode))
            .put("offset", Json::uint(self.offset))
            .put("bytes", self.bytes.encode())
            .put("final", Json::Bool(self.final_chunk))
            .build()
    }
}

impl FileReplace {
    pub(crate) fn decode(value: &Json) -> Result<Self, Refusal> {
        let mut f = Fields::of(value)?;
        let replace = Self {
            resource: Token::decode(f.req("resource")?)?,
            precondition: Token::decode(f.req("precondition")?)?,
            upload: Token::decode(f.req("upload")?)?,
            action_id: Token::decode(f.req("action_id")?)?,
        };
        f.finish()?;
        Ok(replace)
    }

    pub(crate) fn encode_without_action(&self) -> Obj {
        Obj::new()
            .put("resource", self.resource.encode())
            .put("precondition", self.precondition.encode())
            .put("upload", self.upload.encode())
    }
}

impl GitAddIgnore {
    pub(crate) fn decode(value: &Json) -> Result<Self, Refusal> {
        let mut f = Fields::of(value)?;
        constant(f.req("operation")?, "git.add-ignore")?;
        let mut args = Fields::of(f.req("args")?)?;
        let run = Self {
            resource: Token::decode(args.req("resource")?)?,
            precondition: Token::decode(args.req("precondition")?)?,
            repository: Token::decode(args.req("repository")?)?,
            action_id: Token::decode(f.req("action_id")?)?,
        };
        args.finish()?;
        f.finish()?;
        Ok(run)
    }

    pub(crate) fn encode_without_action(&self) -> Obj {
        let args = Obj::new()
            .put("resource", self.resource.encode())
            .put("precondition", self.precondition.encode())
            .put("repository", self.repository.encode())
            .build();
        Obj::new()
            .put("operation", Json::str("git.add-ignore"))
            .put("args", args)
    }
}

impl StateWrite {
    pub(crate) fn decode(value: &Json) -> Result<Self, Refusal> {
        let mut f = Fields::of(value)?;
        let write = Self {
            handle: Token::decode(f.req("handle")?)?,
            precondition: Token::decode(f.req("precondition")?)?,
            bytes: Chunk::decode(f.req("bytes")?)?,
            provenance: unique_list(f.req("provenance")?, 0, 128, Token::decode)?,
            action_id: Token::decode(f.req("action_id")?)?,
        };
        f.finish()?;
        Ok(write)
    }

    pub(crate) fn encode_without_action(&self) -> Obj {
        Obj::new()
            .put("handle", self.handle.encode())
            .put("precondition", self.precondition.encode())
            .put("bytes", self.bytes.encode())
            .put("provenance", array(&self.provenance, Token::encode))
    }
}

impl StateReclaim {
    pub(crate) fn decode(value: &Json) -> Result<Self, Refusal> {
        let mut f = Fields::of(value)?;
        let reclaim = Self {
            handle: Token::decode(f.req("handle")?)?,
            precondition: Token::decode(f.req("precondition")?)?,
            action_id: Token::decode(f.req("action_id")?)?,
        };
        f.finish()?;
        Ok(reclaim)
    }

    pub(crate) fn encode_without_action(&self) -> Obj {
        Obj::new()
            .put("handle", self.handle.encode())
            .put("precondition", self.precondition.encode())
    }
}

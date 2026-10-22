//! Shared test support: the PL-C fixture corpus, a fake broker with spy counters, and fixture builders.
//! Implements the fixtures/README.md rules (exact bytes, null input = absent or withheld) and the fake
//! host-broker semantics of PLUGINS.md §3.3 (preconditions, sealed uploads, replay keys, receipts).
//! Depends on dctx-plugin-protocol and the plugin library; used by every integration test here.

#![allow(dead_code)]

pub mod cases;
pub mod host;

use std::fmt::Write as _;
use std::collections::BTreeMap;
use std::path::PathBuf;

use dctx_plugin_git::app::binding::{Binding, bind};
use dctx_plugin_git::ports::{Broker, BrokerFailure, ProgramRan, Replaced, Uploaded};
use dctx_plugin_protocol::records::broker::{ChunkRead, FileReplace, FileStage, GitAddIgnore};
use dctx_plugin_protocol::records::bytes::Chunk;
use dctx_plugin_protocol::records::common::{ConfigValue, Resource, ResourceKind, Scalar};
use dctx_plugin_protocol::records::envelope::{SessionBody, SessionMessage};
use dctx_plugin_protocol::records::events::ProgressUnit;
use dctx_plugin_protocol::records::invocation::{HookPayload, Invocation};
use dctx_plugin_protocol::records::requests::{Initialize, Request};
use dctx_plugin_protocol::records::results::{ChunkResult, ProgramStatus};
use dctx_plugin_protocol::records::scalars::{Name, Role, SemVer, Token};
use dctx_plugin_protocol::{DecodeLimits, EffectState, ErrorCode, Json, Phase, WireError, WireMessage, decode_object};

pub const CORPUS: DecodeLimits = DecodeLimits { max_bytes: 4 << 20, max_depth: 64, max_members: 1 << 20 };

pub fn api_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/design/plugin-api")
}

pub fn bytes(relative: &str) -> Vec<u8> {
    let path = api_dir().join(relative);
    std::fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

pub fn document(relative: &str) -> Json {
    decode_object(&bytes(relative), CORPUS).unwrap_or_else(|refusal| panic!("{relative}: {refusal}"))
}

pub fn text<'a>(value: &'a Json, key: &str) -> &'a str {
    value.get(key).and_then(Json::as_str).unwrap_or("")
}

pub fn token(text: &str) -> Token {
    Token::new(text).expect("token")
}

pub fn session_request(relative: &str) -> Request {
    match WireMessage::decode(&bytes(relative), DecodeLimits::WIRE).expect(relative) {
        WireMessage::Session(SessionMessage { body: SessionBody::Request { request, .. }, .. }) => request,
        other => panic!("{relative}: {other:?}"),
    }
}

pub fn initialize() -> Initialize {
    match session_request("fixtures/valid/initialize.json") {
        Request::Initialize(init) => *init,
        other => panic!("{other:?}"),
    }
}

pub fn invocation(relative: &str) -> Invocation {
    match session_request(relative) {
        Request::Invoke(invocation) => *invocation,
        other => panic!("{other:?}"),
    }
}

/// The before-pack hook invocation for `host_version`, `stage` and an optional repository.
pub fn hook(host_version: &str, stage: bool, repository: bool) -> Invocation {
    let mut invocation = invocation("fixtures/valid/invoke-before-pack-output-written.json");
    invocation.host_version = SemVer::new(host_version).expect("semver");
    invocation.config.insert(Name::new("stage").expect("name"), ConfigValue::Scalar(Scalar::Bool(stage)));
    if let HookPayload::BeforePackOutputWritten { repository: bound, .. } = &mut invocation.payload
        && !repository {
            *bound = None;
        }
    invocation
}

/// The fixture initialize, with the ignore resource removed when the file is absent and nested rules added.
pub fn binding(ignore_exists: bool, nested: &[&str], program_api: bool) -> Binding {
    let mut init = initialize();
    if !ignore_exists {
        init.resources.retain(|resource| resource.handle.as_str() != "file:ignore");
    }
    for handle in nested {
        init.resources.push(Resource {
            handle: token(handle),
            kind: ResourceKind::File,
            role: Some(Role::new("nested-ignore").expect("role")),
            bytes: None,
        });
    }
    bind(&init, program_api).expect("bind")
}

/// How the fake Git recipe answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitScript {
    Succeed,
    Exit(u32),
    Missing,
    Refuse(ErrorCode, EffectState),
}

fn host_error(code: ErrorCode, reason: &str, effect: EffectState) -> BrokerFailure {
    let mut error = WireError::refusal(code, Phase::Broker, Name::new(reason).expect("name"));
    error.effect_state = effect;
    BrokerFailure::Refused(Box::new(error))
}

/// An in-memory host broker for one project root with spy counters.
#[derive(Debug)]
pub struct FakeBroker {
    pub file: Vec<u8>,
    pub readable: bool,
    pub nested: BTreeMap<String, Option<Vec<u8>>>,
    pub generation: u64,
    pub git: GitScript,
    pub concurrent_edit: Option<Vec<u8>>,
    pub staged: Option<Vec<u8>>,
    pub reads: u64,
    pub hashes: u64,
    pub writes: u64,
    pub git_spawns: u64,
    pub log: Vec<String>,
    uploads: BTreeMap<String, (Token, Vec<u8>, bool)>,
    actions: BTreeMap<String, (String, Token)>,
    receipts: u64,
}

impl FakeBroker {
    pub fn new(file: &[u8]) -> Self {
        Self {
            file: file.to_vec(),
            readable: true,
            nested: BTreeMap::new(),
            generation: 1,
            git: GitScript::Succeed,
            concurrent_edit: None,
            staged: None,
            reads: 0,
            hashes: 0,
            writes: 0,
            git_spawns: 0,
            log: Vec::new(),
            uploads: BTreeMap::new(),
            actions: BTreeMap::new(),
            receipts: 0,
        }
    }

    fn precondition(&self) -> Token {
        token(&format!("pre:{}", self.generation))
    }

    fn receipt(&mut self, kind: &str) -> Token {
        self.receipts += 1;
        token(&format!("receipt:{kind}.{}", self.receipts))
    }

    fn replay(&mut self, action: &Token, arguments: &str) -> Result<Option<Token>, BrokerFailure> {
        match self.actions.get(action.as_str()) {
            Some((recorded, receipt)) if *recorded == arguments => Ok(Some(receipt.clone())),
            Some(_) => Err(host_error(ErrorCode::Conflict, "replay-arguments-differ", EffectState::None)),
            None => Ok(None),
        }
    }
}

impl Broker for FakeBroker {
    fn file_read(&mut self, read: &ChunkRead) -> Result<ChunkResult, BrokerFailure> {
        self.log.push(format!("file.read {} {}", read.handle.as_str(), read.offset));
        let content = match read.handle.as_str() {
            "file:ignore" if self.readable => self.file.clone(),
            "file:ignore" => return Err(host_error(ErrorCode::Denied, "selection-denied", EffectState::None)),
            other => match self.nested.get(other) {
                Some(Some(bytes)) => bytes.clone(),
                _ => return Err(host_error(ErrorCode::Denied, "selection-denied", EffectState::None)),
            },
        };
        if read.precondition.as_ref().is_some_and(|bound| *bound != self.precondition()) {
            return Err(host_error(ErrorCode::Conflict, "precondition-changed", EffectState::None));
        }
        let start = usize::try_from(read.offset).expect("offset").min(content.len());
        let end = content.len().min(start + usize::try_from(read.length).expect("length"));
        self.reads += 1;
        Ok(ChunkResult {
            bytes: Chunk::new(&content[start..end]).expect("chunk"),
            offset: read.offset,
            size: content.len() as u64,
            eof: end == content.len(),
            precondition: Some(self.precondition()),
            receipt: self.receipt("read"),
        })
    }

    fn file_stage(&mut self, stage: &FileStage) -> Result<Uploaded, BrokerFailure> {
        self.log.push(format!("file.stage {}", stage.offset));
        if stage.precondition != self.precondition() {
            return Err(host_error(ErrorCode::Conflict, "precondition-changed", EffectState::None));
        }
        let id = match &stage.upload {
            Some(upload) => upload.as_str().to_owned(),
            None => format!("upload:{}", self.uploads.len() + 1),
        };
        let entry = self.uploads.entry(id.clone()).or_insert((stage.precondition.clone(), Vec::new(), false));
        if entry.2 || entry.1.len() as u64 != stage.offset {
            return Err(host_error(ErrorCode::Conflict, "upload-offset", EffectState::None));
        }
        entry.1.extend_from_slice(stage.bytes.as_bytes());
        entry.2 = stage.final_chunk;
        Ok(Uploaded { upload: token(&id), next_offset: entry.1.len() as u64, complete: entry.2 })
    }

    fn file_replace(&mut self, replace: &FileReplace) -> Result<Replaced, BrokerFailure> {
        self.log.push("file.replace".into());
        let arguments = format!("{}|{}|{}", replace.resource.as_str(), replace.precondition.as_str(), replace.upload.as_str());
        if let Some(receipt) = self.replay(&replace.action_id, &arguments)? {
            return Ok(Replaced { precondition: self.precondition(), receipt });
        }
        if let Some(edit) = self.concurrent_edit.take() {
            self.file = edit;
            self.generation += 1;
        }
        let Some((bound, bytes, true)) = self.uploads.get(replace.upload.as_str()).cloned() else {
            return Err(host_error(ErrorCode::Conflict, "upload-not-sealed", EffectState::None));
        };
        if bound != replace.precondition || replace.precondition != self.precondition() {
            return Err(host_error(ErrorCode::Conflict, "precondition-changed", EffectState::None));
        }
        self.file = bytes;
        self.generation += 1;
        self.writes += 1;
        self.uploads.remove(replace.upload.as_str());
        let receipt = self.receipt("replace");
        self.actions.insert(replace.action_id.as_str().to_owned(), (arguments, receipt.clone()));
        Ok(Replaced { precondition: self.precondition(), receipt })
    }

    fn program_run(&mut self, run: &GitAddIgnore) -> Result<ProgramRan, BrokerFailure> {
        self.log.push("program.run".into());
        if run.resource.as_str() != "file:ignore" || run.repository.as_str() != "repository:1" {
            return Err(host_error(ErrorCode::Denied, "wrong-binding", EffectState::None));
        }
        if run.precondition != self.precondition() {
            return Err(host_error(ErrorCode::Conflict, "staging-precondition", EffectState::None));
        }
        match self.git.clone() {
            GitScript::Missing => Err(host_error(ErrorCode::ExternalProgramMissing, "git-missing", EffectState::None)),
            GitScript::Refuse(code, effect) => Err(host_error(code, "refused", effect)),
            GitScript::Succeed => {
                self.git_spawns += 1;
                self.staged = Some(self.file.clone());
                Ok(ProgramRan { status: ProgramStatus::Succeeded, exit_code: 0, receipt: self.receipt("stage") })
            }
            GitScript::Exit(code) => {
                self.git_spawns += 1;
                Ok(ProgramRan { status: ProgramStatus::Failed, exit_code: code, receipt: self.receipt("stage") })
            }
        }
    }

    fn progress(&mut self, phase: &'static str, completed: u64, _total: Option<u64>, _unit: ProgressUnit) {
        self.log.push(format!("progress {phase} {completed}"));
    }
}

/// Lowercase hex of a SHA-256 computed with the first-party dctx-hash crate.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = dctx_hash::Sha256::digest(bytes).expect("digest");
    let mut text = String::with_capacity(64);
    for byte in digest.as_bytes() { let _ = write!(text, "{byte:02x}"); }
    text
}

//! Shared fixture: a ready broker over fake audit, file and recipe ports with `.gitignore = "abc"`.
//! Used by every integration test file; positive controls start from this fully granted state.

#![allow(dead_code)]

use dctx_plugin_host::fakes::{FakeAudit, FakeFiles, FakeProcess};
use dctx_plugin_host::{Authority, Binding, Broker, Budget, Decision, GrantLayers, HandleKind, Ledger, Reply, Selection};
use dctx_plugin_protocol::records::broker::{ChunkRead, FileReplace, FileStage, GitAddIgnore};
use dctx_plugin_protocol::records::bytes::Chunk;
use dctx_plugin_protocol::records::capability::{Capability, OutputOperation};
use dctx_plugin_protocol::records::requests::Request;
use dctx_plugin_protocol::records::scalars::{Digest, PluginId, ProjectPath, Role, Token};

pub type TestBroker = Broker<FakeAudit, FakeFiles, FakeProcess>;

pub fn path() -> ProjectPath {
    ProjectPath::new(".gitignore").expect("path")
}

pub fn token(text: &str) -> Token {
    Token::new(text).expect("token")
}

pub fn binding(invocation: &str) -> Binding {
    Binding {
        plugin: PluginId::new("dctx/git").expect("id"),
        artifact: Digest::new(&format!("sha256:{}", "a".repeat(64))).expect("digest"),
        root: token("project:1"),
        invocation: token(invocation),
        session: token("s:1"),
        generation: 1,
        namespace: token("ns:git"),
    }
}

pub fn read_cap() -> Capability {
    Capability::FileRead { path: path(), max_bytes: 1 << 20 }
}

pub fn write_cap() -> Capability {
    Capability::FileWrite { path: path(), create: false, max_growth_bytes: 4096, max_bytes: 1 << 20 }
}

pub fn git_cap() -> Capability {
    Capability::GitAddIgnore { max_output_bytes: 65_536 }
}

pub fn outputs_cap() -> Capability {
    Capability::OutputsRead {
        roles: vec![Role::new("pack-text").expect("role")],
        operations: vec![OutputOperation::Pack],
        max_bytes: 1 << 20,
    }
}

pub fn all_caps() -> Vec<Capability> {
    vec![read_cap(), write_cap(), git_cap(), outputs_cap()]
}

pub fn layers(organization: Vec<Capability>) -> GrantLayers {
    let all = all_caps();
    GrantLayers { manifest: all.clone(), consent: all.clone(), organization, host: all.clone(), adapter: all, denies: vec![] }
}

pub fn included() -> Selection {
    Selection::new([(path(), Decision::Include), (ProjectPath::new("src/a.rs").expect("p"), Decision::Include)])
}

pub fn budget(content: u64, actions: u64) -> Budget {
    Budget::new(&[content], &[actions], &[1 << 20], &[65_536], &[16])
}

pub fn broker_with(layers: GrantLayers, selection: Selection, budget: Budget) -> TestBroker {
    let mut authority = Authority::new(layers, selection, binding("op:1"), 0);
    authority.initialize().expect("ready");
    let mut files = FakeFiles::default();
    files.put(&path(), b"abc");
    Broker::new(authority, Ledger::new(64), budget, 1 << 20, (FakeAudit::default(), files, FakeProcess::default()))
}

pub fn broker() -> TestBroker {
    broker_with(layers(all_caps()), included(), budget(8 << 20, 8))
}

pub fn file_handle(broker: &mut TestBroker) -> Token {
    broker.register(HandleKind::File { path: path() }).expect("handle")
}

pub fn read_req(handle: &Token, offset: u64, length: u64, precondition: Option<Token>) -> Request {
    Request::FileRead(ChunkRead { handle: handle.clone(), offset, length, precondition })
}

/// Reads the first chunk and returns its precondition token.
pub fn first_read(broker: &mut TestBroker, handle: &Token) -> Token {
    match broker.handle(&read_req(handle, 0, 3, None)) {
        Ok(Reply::Chunk { precondition: Some(pre), .. }) => pre,
        other => panic!("first read: {other:?}"),
    }
}

/// Stages `bytes` as one sealed upload and returns the upload token.
pub fn stage(broker: &mut TestBroker, handle: &Token, precondition: &Token, bytes: &[u8]) -> Token {
    let request = Request::FileStage(FileStage {
        resource: handle.clone(),
        precondition: precondition.clone(),
        upload: None,
        offset: 0,
        bytes: Chunk::new(bytes).expect("chunk"),
        final_chunk: true,
    });
    match broker.handle(&request) {
        Ok(Reply::Staged { upload, sealed: true }) => upload,
        other => panic!("stage: {other:?}"),
    }
}

pub fn replace_req(handle: &Token, precondition: &Token, upload: &Token, action: &str) -> Request {
    Request::FileReplace(FileReplace {
        resource: handle.clone(),
        precondition: precondition.clone(),
        upload: upload.clone(),
        action_id: token(action),
    })
}

pub fn git_req(handle: &Token, precondition: &Token, repository: &Token, action: &str) -> Request {
    Request::ProgramRun(GitAddIgnore {
        resource: handle.clone(),
        precondition: precondition.clone(),
        repository: repository.clone(),
        action_id: token(action),
    })
}

/// Full read → stage → replace path; returns the reply.
pub fn replace_with(broker: &mut TestBroker, bytes: &[u8], action: &str) -> Result<Reply, dctx_plugin_host::HostRefusal> {
    let handle = file_handle(broker);
    let pre = first_read(broker, &handle);
    let upload = stage(broker, &handle, &pre, bytes);
    broker.handle(&replace_req(&handle, &pre, &upload, action))
}

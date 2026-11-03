// SPDX-License-Identifier: Apache-2.0
//! Scripted broker effects for one hook invocation.
//! Implements PLUGINS.md §3.3 file and `git.add-ignore` calls through the host broker.
//! Depends on [`super::ports::LiveSession`]. [`super::boundary`] decides whether the hook succeeded.
//! The script never opens a project path or a program itself.

use dctx_plugin_host::{HandleKind, HostRefusal, Reply};
use dctx_plugin_protocol::records::broker::{ChunkRead, FileReplace, FileStage, GitAddIgnore};
use dctx_plugin_protocol::records::bytes::Chunk;
use dctx_plugin_protocol::records::requests::Request;
use dctx_plugin_protocol::records::scalars::{ProjectPath, Token};
use dctx_plugin_protocol::{ErrorCode, Phase};

use super::ports::{bad_token, LiveSession};
use super::{HookEffect, PluginHost};

pub(crate) fn apply(session: &mut LiveSession, host: &mut PluginHost, effects: &[HookEffect], now: u64) -> Result<(), HostRefusal> {
    let path = ProjectPath::new(".gitignore").map_err(|_| bad_token())?;
    let file = session.broker_mut().register(HandleKind::File { path })?;
    let repository = session.broker_mut().register(HandleKind::Repository)?;
    for effect in effects {
        match effect {
            HookEffect::ReadGitignore => {
                let _ = read_ignore(session, host, &file, now)?;
            }
            HookEffect::ReplaceGitignore(bytes) => replace_ignore(session, host, &file, bytes, now)?,
            HookEffect::StageGitignore => stage_ignore(session, host, &file, &repository, now)?,
            HookEffect::Fail => return Err(refused("hook-failed")),
            HookEffect::Warn => return Err(refused("hook-warning")),
        }
    }
    Ok(())
}

fn read_ignore(session: &mut LiveSession, host: &mut PluginHost, file: &Token, now: u64) -> Result<Token, HostRefusal> {
    let request = Request::FileRead(ChunkRead { handle: file.clone(), offset: 0, length: 64, precondition: None });
    match call(session, host, request, now)? {
        Reply::Chunk { precondition: Some(token), .. } => Ok(token),
        _ => Err(HostRefusal::new(ErrorCode::InvalidMessage, Phase::Broker, "hook-read")),
    }
}

fn replace_ignore(session: &mut LiveSession, host: &mut PluginHost, file: &Token, bytes: &[u8], now: u64) -> Result<(), HostRefusal> {
    let precondition = read_ignore(session, host, file, now)?;
    let staged = Request::FileStage(FileStage {
        resource: file.clone(),
        precondition: precondition.clone(),
        upload: None,
        offset: 0,
        bytes: Chunk::new(bytes).map_err(|_| HostRefusal::limit("chunk"))?,
        final_chunk: true,
    });
    let upload = match call(session, host, staged, now)? {
        Reply::Staged { upload, sealed: true } => upload,
        _ => return Err(HostRefusal::new(ErrorCode::InvalidMessage, Phase::Broker, "hook-stage")),
    };
    let action_id = host.next_token("a")?;
    let replace = Request::FileReplace(FileReplace { resource: file.clone(), precondition, upload, action_id });
    match call(session, host, replace, now)? {
        Reply::Receipt { .. } => Ok(()),
        _ => Err(HostRefusal::new(ErrorCode::InvalidMessage, Phase::Broker, "hook-replace")),
    }
}

fn stage_ignore(session: &mut LiveSession, host: &mut PluginHost, file: &Token, repository: &Token, now: u64) -> Result<(), HostRefusal> {
    let precondition = read_ignore(session, host, file, now)?;
    let action_id = host.next_token("a")?;
    let request = Request::ProgramRun(GitAddIgnore {
        resource: file.clone(),
        precondition,
        repository: repository.clone(),
        action_id,
    });
    match call(session, host, request, now)? {
        Reply::Receipt { .. } => Ok(()),
        _ => Err(HostRefusal::new(ErrorCode::InvalidMessage, Phase::Broker, "hook-stage-git")),
    }
}

fn call(session: &mut LiveSession, host: &mut PluginHost, request: Request, now: u64) -> Result<Reply, HostRefusal> {
    let id = host.next_request()?;
    session.receive(id, request, now)?;
    match session.serve_next(now) {
        Some((_, result)) => result,
        None => Err(HostRefusal::new(ErrorCode::InvalidMessage, Phase::Broker, "request-not-served")),
    }
}

fn refused(reason: &'static str) -> HostRefusal {
    HostRefusal::new(ErrorCode::ExternalProgramFailed, Phase::Invoke, reason)
}

//! The worker lifecycle: hello -> initialize -> ready -> invoke* -> shutdown, over inherited stdio.
//! Implements PLUGINS.md §3.1 lifecycle and wire.md §Bootstrap and lifecycle for the dctx/git worker:
//! highest common version, identity from the launch record, required APIs checked before selection,
//! cancelled event then a typed Cancelled error. Same code for the standalone and same-binary worker.
//! Depends on adapters::{link, wire_broker}, app::{binding, invoke}; used by main.rs and conformance tests.

use std::io::{Read, Write};

use dctx_plugin_protocol::records::bootstrap::BootstrapMessage;
use dctx_plugin_protocol::records::common::Identity;
use dctx_plugin_protocol::records::envelope::SessionBody;
use dctx_plugin_protocol::records::events::Event;
use dctx_plugin_protocol::records::requests::Request;
use dctx_plugin_protocol::records::results::ResultBody;
use dctx_plugin_protocol::records::scalars::{Digest, Name, PluginId, SemVer, Token};
use dctx_plugin_protocol::{
    BOOTSTRAP_FRAME_CAP, DecodeLimits, ErrorCode, FrameReader, Phase, Refusal, SUPPORTED_VERSIONS, Session,
    WireError, WireMessage, WorkerProfile, accept_answer, answer_hello, write_frame,
};

use crate::adapters::link::{Link, stream};
use crate::adapters::wire_broker::WireBroker;
use crate::adapters::{WorkerError, violation};
use crate::app::binding::{Binding, bind};
use crate::app::invoke::{invoke, result};

/// Canonical plugin ID.
pub const PLUGIN_ID: &str = "dctx/git";
/// APIs the worker cannot run without.
pub const REQUIRED_APIS: [&str; 3] = ["file.read", "file.stage", "file.replace"];
/// APIs the worker uses when offered.
pub const OPTIONAL_APIS: [&str; 1] = ["program.run"];

/// The launch identity the host supplies (its verified install record); never self-asserted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchIdentity {
    /// Pinned publisher key ID.
    pub publisher_key: Token,
    /// Verified artifact digest.
    pub artifact_digest: Digest,
}

impl LaunchIdentity {
    /// Parses exactly `--publisher-key <token> --artifact-digest <sha256:...>` (either order).
    pub fn from_args(args: &[String]) -> Result<Self, &'static str> {
        let (mut key, mut digest) = (None, None);
        for pair in args.chunks(2) {
            match pair {
                [flag, value] if flag == "--publisher-key" && key.is_none() => {
                    key = Some(Token::new(value).map_err(|_| "invalid-publisher-key")?);
                }
                [flag, value] if flag == "--artifact-digest" && digest.is_none() => {
                    digest = Some(Digest::new(value).map_err(|_| "invalid-artifact-digest")?);
                }
                _ => return Err("usage: --publisher-key <token> --artifact-digest <sha256:hex>"),
            }
        }
        match (key, digest) {
            (Some(publisher_key), Some(artifact_digest)) => Ok(Self { publisher_key, artifact_digest }),
            _ => Err("usage: --publisher-key <token> --artifact-digest <sha256:hex>"),
        }
    }

    /// The handshake identity: canonical ID and this crate's independent plugin version.
    pub fn identity(&self) -> Result<Identity, Refusal> {
        Ok(Identity {
            id: PluginId::new(PLUGIN_ID)?,
            version: SemVer::new(env!("CARGO_PKG_VERSION"))?,
            publisher_key: self.publisher_key.clone(),
            artifact_digest: self.artifact_digest.clone(),
        })
    }
}

fn names(items: &[&str]) -> Result<Vec<Name>, Refusal> {
    items.iter().map(|item| Name::new(item)).collect()
}

fn negotiate<R: Read, W: Write>(
    reader: &mut FrameReader<R>,
    writer: &mut W,
    identity: &Identity,
) -> Result<dctx_plugin_protocol::Negotiated, WorkerError> {
    let frame = reader.next_frame().map_err(stream)?.ok_or(WorkerError::Closed)?;
    let message = WireMessage::decode(&frame, DecodeLimits::BOOTSTRAP).map_err(WorkerError::Protocol)?;
    let WireMessage::Bootstrap(BootstrapMessage::Hello { id, hello }) = message else {
        return Err(WorkerError::Protocol(violation("expected-hello")));
    };
    let profile = WorkerProfile {
        supported: SUPPORTED_VERSIONS.to_vec(),
        identity: identity.clone(),
        required_apis: names(&REQUIRED_APIS).map_err(WorkerError::Internal)?,
        optional_apis: names(&OPTIONAL_APIS).map_err(WorkerError::Internal)?,
    };
    let mut answer = answer_hello(&id, &hello, &profile).map_err(WorkerError::Protocol)?;
    if matches!(answer, BootstrapMessage::Selected { .. }) && profile.required_apis.iter().any(|api| !hello.apis.contains(api)) {
        let reason = Name::new("required-api-unoffered").map_err(WorkerError::Internal)?;
        let error = WireError::refusal(ErrorCode::UnsupportedCapability, Phase::Negotiate, reason);
        answer = BootstrapMessage::Refused { id: id.clone(), error: Box::new(error) };
    }
    write_frame(writer, &WireMessage::Bootstrap(answer.clone()).encode(), BOOTSTRAP_FRAME_CAP).map_err(stream)?;
    if let BootstrapMessage::Refused { error, .. } = &answer {
        return Err(WorkerError::Unsupported(error.code));
    }
    let negotiated = accept_answer(&id, &hello, &answer, identity, &[]).map_err(WorkerError::Internal)?;
    reader.decoder_mut().set_cap(negotiated.frame_cap).map_err(WorkerError::Protocol)?;
    Ok(negotiated)
}

/// Serves one session; `Ok` after shutdown or a clean end of stream between invocations.
pub fn serve<R: Read, W: Write>(reader: R, mut writer: W, launch: &LaunchIdentity) -> Result<(), WorkerError> {
    let identity = launch.identity().map_err(WorkerError::Internal)?;
    let mut reader = FrameReader::new(reader);
    let negotiated = negotiate(&mut reader, &mut writer, &identity)?;
    let program_api = negotiated.apis.iter().any(|api| api.as_str() == "program.run");
    let mut link = Link::new(reader, writer);
    let mut session = Some(negotiated);
    let mut binding: Option<Binding> = None;
    loop {
        let Some(message) = link.recv()? else { return Ok(()) };
        if let Some(negotiated) = session.take() {
            link.start(message.session.clone(), Session::new(message.session.clone(), negotiated));
        }
        link.observe_host(&message)?;
        let SessionBody::Request { id, request } = message.body else {
            return Err(WorkerError::Protocol(violation("unexpected-host-message")));
        };
        match request {
            Request::Initialize(init) => match bind(&init, program_api) {
                Ok(bound) => {
                    binding = Some(bound);
                    link.respond(id, &ResultBody::Ready)?;
                }
                Err(failure) => {
                    let code = failure.code;
                    link.respond_error(id, failure.to_wire().map_err(WorkerError::Internal)?)?;
                    return Err(WorkerError::InitializeRefused(code));
                }
            },
            Request::Invoke(invocation) => {
                let bound = binding.as_ref().ok_or(WorkerError::Protocol(violation("invoke-before-ready")))?;
                link.begin(invocation.invocation.clone());
                let mut broker = WireBroker::new(&mut link);
                let outcome = invoke(&mut broker, bound, &invocation);
                if let Some(fatal) = broker.fatal.take() {
                    return Err(fatal);
                }
                match outcome {
                    Ok(answer) => link.respond(id, &result(&answer).map_err(WorkerError::Internal)?)?,
                    Err(failure) => {
                        if link.cancelled {
                            let last_receipt = failure.receipts.last().cloned();
                            link.event(Event::Cancelled { invocation: invocation.invocation.clone(), last_receipt })?;
                        }
                        link.respond_error(id, failure.to_wire().map_err(WorkerError::Internal)?)?;
                    }
                }
                link.invocation = None;
            }
            Request::Shutdown { .. } => {
                link.respond(id, &ResultBody::Closing)?;
                return Ok(());
            }
            _ => return Err(WorkerError::Protocol(violation("unexpected-host-request"))),
        }
    }
}

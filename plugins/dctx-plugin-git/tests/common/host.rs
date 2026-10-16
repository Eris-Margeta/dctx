//! A scripted fake host driving the real worker over in-memory framed pipes, with its own session tracker.
//! Implements the host side of wire.md §Bootstrap and lifecycle for standalone protocol conformance:
//! every worker frame is strictly decoded and observed by dctx-plugin-protocol's `Session` (a refusal fails
//! the test); broker requests are answered from the fake broker. Used by conformance.rs.

use std::io::{self, Read, Write};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::{self, JoinHandle};

use dctx_plugin_git::adapters::WorkerError;
use dctx_plugin_git::adapters::worker::{LaunchIdentity, serve};
use dctx_plugin_git::ports::{Broker, BrokerFailure};
use dctx_plugin_protocol::records::bootstrap::BootstrapMessage;
use dctx_plugin_protocol::records::envelope::{Outcome, PendingResult, SessionBody, SessionMessage};
use dctx_plugin_protocol::records::requests::Request;
use dctx_plugin_protocol::records::results::ResultBody;
use dctx_plugin_protocol::records::scalars::{Digest, RequestId, Sender as Side};
use dctx_plugin_protocol::{
    Accepted, BOOTSTRAP_FRAME_CAP, DecodeLimits, FrameReader, MAX_FRAME_CAP, Session, WireMessage, accept_answer,
    write_frame,
};

use super::{FakeBroker, bytes, token};

pub struct PipeReader {
    rx: Receiver<Vec<u8>>,
    buffer: Vec<u8>,
    position: usize,
}

impl Read for PipeReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.position == self.buffer.len() {
            match self.rx.recv() {
                Ok(chunk) => {
                    self.buffer = chunk;
                    self.position = 0;
                }
                Err(_) => return Ok(0),
            }
        }
        let count = out.len().min(self.buffer.len() - self.position);
        out[..count].copy_from_slice(&self.buffer[self.position..self.position + count]);
        self.position += count;
        Ok(count)
    }
}

pub struct PipeWriter(Sender<Vec<u8>>);

impl Write for PipeWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.0.send(data.to_vec()).map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))?;
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn pipe() -> (PipeWriter, PipeReader) {
    let (tx, rx) = channel();
    (PipeWriter(tx), PipeReader { rx, buffer: Vec::new(), position: 0 })
}

pub fn launch() -> LaunchIdentity {
    LaunchIdentity {
        publisher_key: token("fixture-key-1"),
        artifact_digest: Digest::new("sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad").expect("digest"),
    }
}

/// The host end of one worker session.
pub struct Host {
    to_worker: Option<PipeWriter>,
    from_worker: FrameReader<PipeReader>,
    worker: Option<JoinHandle<Result<(), WorkerError>>>,
    pub session: Option<Session>,
    next_id: u64,
}

impl Host {
    pub fn spawn() -> Self {
        let (to_worker, worker_in) = pipe();
        let (worker_out, from_worker) = pipe();
        let worker = thread::spawn(move || serve(worker_in, worker_out, &launch()));
        Self { to_worker: Some(to_worker), from_worker: FrameReader::new(from_worker), worker: Some(worker), session: None, next_id: 0 }
    }

    pub fn send_raw(&mut self, body: &[u8], cap: u32) {
        write_frame(self.to_worker.as_mut().expect("open"), body, cap).expect("write");
    }

    pub fn read_raw(&mut self) -> Option<Vec<u8>> {
        self.from_worker.next_frame().expect("frame")
    }

    /// Sends a hello body and returns the worker's bootstrap answer.
    pub fn hello(&mut self, hello: &[u8]) -> BootstrapMessage {
        self.send_raw(hello, BOOTSTRAP_FRAME_CAP);
        let answer = self.read_raw().expect("answer");
        match WireMessage::decode(&answer, DecodeLimits::BOOTSTRAP).expect("bootstrap answer") {
            WireMessage::Bootstrap(message) => message,
            other => panic!("{other:?}"),
        }
    }

    /// Fixture hello, accepted against the fixture install identity; starts session `s:1`.
    pub fn negotiate(&mut self) -> BootstrapMessage {
        let hello_bytes = bytes("fixtures/valid/hello.json");
        let WireMessage::Bootstrap(BootstrapMessage::Hello { id, hello }) =
            WireMessage::decode(&hello_bytes, DecodeLimits::BOOTSTRAP).expect("hello")
        else {
            panic!("hello fixture")
        };
        let answer = self.hello(&hello_bytes);
        let identity = launch().identity().expect("identity");
        let negotiated = accept_answer(&id, &hello, &answer, &identity, &[]).expect("accepted");
        self.from_worker.decoder_mut().set_cap(negotiated.frame_cap).expect("cap");
        self.session = Some(Session::new(token("s:1"), negotiated));
        answer
    }

    fn message(&self, body: SessionBody) -> SessionMessage {
        SessionMessage { session: token("s:1"), extensions: None, body }
    }

    /// Sends a tracked host request; returns its ID.
    pub fn request(&mut self, request: Request) -> RequestId {
        self.next_id += 1;
        let id = RequestId::new(&format!("h:{}", self.next_id)).expect("id");
        let message = self.message(SessionBody::Request { id: id.clone(), request });
        self.session.as_mut().expect("session").observe(Side::Host, &message).expect("host request admitted");
        self.send_raw(&WireMessage::Session(message).encode(), MAX_FRAME_CAP);
        id
    }

    /// Next worker message, observed by the host tracker (a refusal fails the test).
    pub fn next(&mut self) -> (SessionMessage, Accepted) {
        let frame = self.read_raw().expect("worker message");
        let WireMessage::Session(message) = WireMessage::decode(&frame, DecodeLimits::WIRE).expect("decodes") else {
            panic!("bootstrap after hello")
        };
        let accepted = self.session.as_mut().expect("session").observe(Side::Plugin, &message).expect("conforms");
        (message, accepted)
    }

    /// Answers broker requests from `broker` until the response to `id` arrives.
    pub fn serve_until(&mut self, id: &RequestId, broker: &mut FakeBroker) -> Accepted {
        loop {
            let (message, accepted) = self.next();
            match message.body {
                SessionBody::Response { id: answered, .. } if &answered == id => return accepted,
                SessionBody::Request { id: request_id, request } => {
                    let outcome = answer(broker, &request);
                    let reply = self.message(SessionBody::Response { id: request_id, outcome });
                    self.session.as_mut().expect("session").observe(Side::Host, &reply).expect("host reply");
                    self.send_raw(&WireMessage::Session(reply).encode(), MAX_FRAME_CAP);
                }
                SessionBody::Event { .. } => {}
                other => panic!("unexpected {other:?}"),
            }
        }
    }

    /// Closes the host end and returns the worker's exit.
    pub fn finish(mut self) -> Result<(), WorkerError> {
        self.to_worker = None;
        self.worker.take().expect("worker").join().expect("worker thread")
    }
}

fn answer(broker: &mut FakeBroker, request: &Request) -> Outcome {
    let result = match request {
        Request::FileRead(read) => broker.file_read(read).map(ResultBody::Chunk),
        Request::FileStage(stage) => broker.file_stage(stage).map(|staged| ResultBody::FileStage {
            upload: staged.upload,
            next_offset: staged.next_offset,
            complete: staged.complete,
            receipt: token("receipt:upload"),
        }),
        Request::FileReplace(replace) => broker
            .file_replace(replace)
            .map(|done| ResultBody::Replaced { precondition: done.precondition, receipt: done.receipt }),
        Request::ProgramRun(run) => broker.program_run(run).map(|ran| ResultBody::ProgramRun {
            status: ran.status,
            exit_code: ran.exit_code,
            diagnostics: Vec::new(),
            receipt: ran.receipt,
        }),
        other => panic!("worker sent a non-broker request {other:?}"),
    };
    match result {
        Ok(body) => Outcome::Result(PendingResult::from_body(&body)),
        Err(BrokerFailure::Refused(error)) => Outcome::Error(error),
        Err(other) => panic!("fake broker failure {other:?}"),
    }
}

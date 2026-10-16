//! Same-binary plugin worker entry (`dctx --internal-plugin-worker`).
//! Implements PLUGINS.md §2: the entry runs before CLI configuration and speaks the external protocol
//! on inherited stdio. Manual invocation gets no broker grants and no project path.
//! Depends on `dctx-plugin-git` only when the `git-worker` feature is on. Used by `main`.

use std::io::{self, Read, Write};

/// Process status for the worker switch.
pub fn enter(args: &[String]) -> u8 {
    match args.first().map(String::as_str) {
        Some("git") if cfg!(feature = "git-worker") => git(&args[1..]),
        Some("git") => {
            eprintln!("plugin worker excluded from this strict build");
            1
        }
        Some(other) => {
            eprintln!("unknown plugin worker: {other}");
            2
        }
        None => {
            eprintln!("usage: dctx --internal-plugin-worker git --publisher-key <token> --artifact-digest <sha256:hex>");
            2
        }
    }
}

fn git(args: &[String]) -> u8 {
    match serve_with(io::stdin().lock(), io::stdout().lock(), args) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("dctx --internal-plugin-worker: {error}");
            if error.starts_with("usage:") { 2 } else { 1 }
        }
    }
}

/// Runs the git worker on `reader`/`writer`. The strict build refuses before any frame is read.
pub fn serve_with<R: Read, W: Write>(reader: R, writer: W, args: &[String]) -> Result<(), String> {
    #[cfg(feature = "git-worker")]
    {
        serve_git(reader, writer, args)
    }
    #[cfg(not(feature = "git-worker"))]
    {
        let _ = (reader, writer, args);
        Err("strict build excludes the git worker".to_owned())
    }
}

#[cfg(feature = "git-worker")]
fn serve_git<R: Read, W: Write>(reader: R, writer: W, args: &[String]) -> Result<(), String> {
    use dctx_plugin_git::adapters::worker::{LaunchIdentity, serve};
    let owned: Vec<String> = args.to_vec();
    let launch = LaunchIdentity::from_args(&owned).map_err(|usage| format!("usage: {usage}"))?;
    serve(reader, writer, &launch).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{Receiver, Sender, channel};
    use std::thread;

    #[test]
    fn strict_feature_does_not_link_the_worker_crate() {
        let manifest = include_str!("../../Cargo.toml");
        assert!(manifest.contains("dctx-plugin-git"));
        assert!(manifest.contains("optional = true"));
        assert!(manifest.contains("git-worker = [\"dep:dctx-plugin-git\"]"));
        assert!(cfg!(feature = "git-worker"));
        assert!(!manifest.contains("git-worker = [\"dep:dctx-plugin-git\", \"dep:dctx-plugin-git\"]"));
    }

    #[test]
    fn manual_entry_without_launch_identity_does_not_start_the_protocol() {
        let error = serve_with(std::io::empty(), std::io::sink(), &[]).unwrap_err();
        assert!(error.contains("usage:"), "{error}");
    }

    #[cfg(feature = "git-worker")]
    #[test]
    fn same_binary_worker_matches_the_standalone_handshake_and_lifecycle() {
        use dctx_plugin_protocol::{DecodeLimits, FrameReader, WireMessage, write_frame, BOOTSTRAP_FRAME_CAP, MAX_FRAME_CAP};
        let (to_worker, worker_in) = pipe();
        let (worker_out, from_worker) = pipe();
        let args = vec![
            "--publisher-key".to_owned(),
            "fixture-key-1".to_owned(),
            "--artifact-digest".to_owned(),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".to_owned(),
        ];
        let worker = thread::spawn(move || serve_with(worker_in, worker_out, &args));
        let mut from_worker = FrameReader::new(from_worker);
        let hello = fixture("fixtures/valid/hello.json");
        write_frame(&mut to_worker.clone_send(), &hello, BOOTSTRAP_FRAME_CAP).unwrap();
        let answer = from_worker.next_frame().unwrap().unwrap();
        let expected = fixture("fixtures/valid/hello-selected.json");
        let got = WireMessage::decode(&answer, DecodeLimits::BOOTSTRAP).unwrap();
        let want = WireMessage::decode(&expected, DecodeLimits::BOOTSTRAP).unwrap();
        assert_eq!(got, want);

        let init = fixture("fixtures/valid/initialize.json");
        write_frame(&mut to_worker.clone_send(), &init, MAX_FRAME_CAP).unwrap();
        let ready = from_worker.next_frame().unwrap().unwrap();
        let ready_message = WireMessage::decode(&ready, DecodeLimits::WIRE).unwrap();
        let expected_ready = WireMessage::decode(&fixture("fixtures/valid/result-initialize.json"), DecodeLimits::WIRE).unwrap();
        assert_eq!(ready_message, expected_ready);

        let shutdown = String::from_utf8(fixture("fixtures/valid/request-shutdown.json")).unwrap().replace("h:1", "h:2");
        write_frame(&mut to_worker.clone_send(), shutdown.as_bytes(), MAX_FRAME_CAP).unwrap();
        let closing = from_worker.next_frame().unwrap().unwrap();
        let closing_text = String::from_utf8(closing).unwrap();
        assert!(closing_text.contains("h:2"), "{closing_text}");
        assert!(closing_text.contains("closing"), "{closing_text}");
        drop(to_worker);
        assert_eq!(worker.join().unwrap(), Ok(()));
    }

    #[cfg(feature = "git-worker")]
    fn fixture(relative: &str) -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/design/plugin-api").join(relative);
        std::fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
    }

    #[cfg(feature = "git-worker")]
    struct PipeWriter(Sender<Vec<u8>>);

    #[cfg(feature = "git-worker")]
    impl PipeWriter {
        fn clone_send(&self) -> Self {
            Self(self.0.clone())
        }
    }

    #[cfg(feature = "git-worker")]
    impl Write for PipeWriter {
        fn write(&mut self, data: &[u8]) -> io::Result<usize> {
            self.0.send(data.to_vec()).map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))?;
            Ok(data.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[cfg(feature = "git-worker")]
    struct PipeReader {
        rx: Receiver<Vec<u8>>,
        buffer: Vec<u8>,
        position: usize,
    }

    #[cfg(feature = "git-worker")]
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

    #[cfg(feature = "git-worker")]
    fn pipe() -> (PipeWriter, PipeReader) {
        let (tx, rx) = channel();
        (PipeWriter(tx), PipeReader { rx, buffer: Vec::new(), position: 0 })
    }
}

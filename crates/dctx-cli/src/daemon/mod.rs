// SPDX-License-Identifier: Apache-2.0
//! Optional local transport for the existing read-only engine transactions.
mod settings;

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use dctx_cli::exit::{CommandOutput, Failure};
use dctx_daemon_transport as transport;
use serde_json::{json, Value};
use settings::Settings;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(2);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_REQUEST: usize = 1024 * 1024;
const MAX_REPLY: usize = 128 * 1024 * 1024;
const POLL: Duration = Duration::from_millis(20);
const HELP: &str = "Usage: dctx daemon status|stop|config on|off\nOptional user-only local daemon for map, read and pack.\nDCTX_DAEMON=1 enables auto-start; DCTX_DAEMON=0 disables it.\nDCTX_DAEMON_IDLE_SECONDS=300 sets the idle exit timeout (1..86400).\nSee docs/DAEMON.md.\n";

pub(crate) fn run(args: &[String]) -> Result<(), Failure> {
    let operation = || -> io::Result<()> {
        match args {
            [word] if word == "status" => {
                let settings = Settings::load()?;
                let reply = exchange_control(&settings.endpoint, "status").ok();
                println!(
                    "{}",
                    json!({"enabled": settings.enabled, "running": reply.is_some(), "idle_seconds": settings.idle.as_secs(), "daemon": reply})
                );
                Ok(())
            }
            [word] if word == "stop" => {
                let settings = Settings::load()?;
                let _ = exchange_control(&settings.endpoint, "stop");
                Ok(())
            }
            [word, enabled] if word == "config" && matches!(enabled.as_str(), "on" | "off") => {
                settings::store(enabled == "on")
            }
            [] => {
                print!("{HELP}");
                Ok(())
            }
            [word] if word == "--help" || word == "-h" => {
                print!("{HELP}");
                Ok(())
            }
            _ => Err(io::Error::new(io::ErrorKind::InvalidInput, HELP)),
        }
    };
    operation().map_err(|error| Failure::usage(error.to_string(), "run `dctx daemon --help`"))
}

/// Returns None on every transport failure, before any output is emitted.
pub(crate) fn try_output(
    args: &[String],
    allow_auto_clean: bool,
) -> Option<Result<CommandOutput, Failure>> {
    if !eligible(args) || !settings::enabled() {
        return None;
    }
    let settings = Settings::load().ok()?;
    if !settings.enabled {
        return None;
    }
    let identity = executable_identity().ok()?;
    let context = context_identity().ok()?;
    let mut stream = ready(&settings, &identity, &context).ok()?;
    write_frame(
        &mut stream,
        &json!({"args": args, "auto_clean":allow_auto_clean}),
    )
    .ok()?;
    let reply = read_frame(&mut stream, MAX_REPLY).ok()?;
    let _ = write_frame(&mut stream, &json!({"op":"ack"}));
    let stdout = reply.get("stdout")?.as_str()?.to_owned();
    let failure = match reply.get("failure")? {
        Value::Null => None,
        value => Some(Failure::new(
            i32::try_from(value.get("code")?.as_i64()?).ok()?,
            value.get("message")?.as_str()?,
            value.get("hint")?.as_str()?,
        )),
    };
    Some(Ok(CommandOutput { stdout, failure }))
}

fn eligible(args: &[String]) -> bool {
    // These switches produce process-local diagnostics on stderr; preserve those bytes in-process.
    if args
        .iter()
        .any(|arg| matches!(arg.as_str(), "--profile" | "--help" | "-h"))
        || ["DCTX_PROFILE", "DCTX_REUSE_REPORT", "DCTX_READ_REPORT"]
            .iter()
            .any(|name| std::env::var(name).ok().as_deref() == Some("1"))
    {
        return false;
    }
    match args.first().map(String::as_str) {
        Some("map" | "read") => true,
        Some("pack") => !args
            .iter()
            .any(|arg| matches!(arg.as_str(), "--output" | "-o" | "--clipboard" | "-c")),
        _ => false,
    }
}

fn execute(args: &[String]) -> Result<CommandOutput, Failure> {
    if !eligible(args) {
        return Err(Failure::usage(
            "unsupported daemon command",
            "run this command in-process",
        ));
    }
    match args.first().map(String::as_str) {
        Some("map") => crate::map::output(&args[1..]),
        Some("read") => crate::map::output(args),
        Some("pack") if crate::pack_for::requested(&args[1..]) => crate::pack_for::output(args),
        Some("pack") => dctx_cli::pack::output(args),
        _ => Err(Failure::usage(
            "unsupported daemon command",
            "use map, read or pack",
        )),
    }
}

fn ready(settings: &Settings, identity: &str, context: &str) -> io::Result<transport::Stream> {
    // Two attempts: existing daemon, then an auto-start/restart. Requests are read-only.
    if let Ok(stream) = handshake(&settings.endpoint, identity, context) {
        return Ok(stream);
    }
    let deadline = Instant::now() + HANDSHAKE_TIMEOUT;
    let mut next_spawn = Instant::now();
    loop {
        if Instant::now() >= next_spawn {
            spawn()?;
            next_spawn = Instant::now() + Duration::from_millis(250);
        }
        if let Ok(stream) = handshake(&settings.endpoint, identity, context) {
            return Ok(stream);
        }
        if Instant::now() >= deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        std::thread::sleep(POLL);
    }
}

fn handshake(path: &Path, identity: &str, context: &str) -> io::Result<transport::Stream> {
    let mut stream = transport::connect(path, HANDSHAKE_TIMEOUT)?;
    write_frame(
        &mut stream,
        &json!({"op":"hello", "identity":identity, "context":context}),
    )?;
    let reply = read_frame(&mut stream, MAX_REQUEST)?;
    if reply.get("identity").and_then(Value::as_str) != Some(identity)
        || reply.get("context").and_then(Value::as_str) != Some(context)
    {
        let _ = write_frame(&mut stream, &json!({"op":"ack"}));
        return Err(io::Error::other(
            "daemon restarting for this executable or environment",
        ));
    }
    transport::set_timeout(&mut stream, COMMAND_TIMEOUT)?;
    Ok(stream)
}

fn spawn() -> io::Result<()> {
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg("--internal-daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0000_0208);
    }
    let mut child = command.spawn()?;
    // Reap children in long-lived callers; a short-lived CLI can exit immediately.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

fn exchange_control(path: &Path, op: &str) -> io::Result<Value> {
    let mut stream = transport::connect(path, HANDSHAKE_TIMEOUT)?;
    write_frame(&mut stream, &json!({"op":op}))?;
    let reply = read_frame(&mut stream, MAX_REQUEST)?;
    let _ = write_frame(&mut stream, &json!({"op":"ack"}));
    Ok(reply)
}

pub(crate) fn serve() -> io::Result<()> {
    let settings = Settings::load()?;
    let mut listener = transport::Listener::bind(&settings.endpoint)?;
    let identity = executable_identity()?;
    let context = context_identity()?;
    let mut last = Instant::now();
    loop {
        match listener.accept(HANDSHAKE_TIMEOUT) {
            Ok(mut stream) => {
                if let Ok(hello) = read_frame(&mut stream, MAX_REQUEST) {
                    let op = hello.get("op").and_then(Value::as_str);
                    let matches = hello.get("identity").and_then(Value::as_str) == Some(&identity)
                        && hello.get("context").and_then(Value::as_str) == Some(&context);
                    let stopping = op == Some("stop") || (op == Some("hello") && !matches);
                    let reply = json!({"identity":identity, "context":context, "version":env!("CARGO_PKG_VERSION"), "pid":std::process::id(), "stopping":stopping});
                    if write_frame(&mut stream, &reply).is_ok() && op == Some("hello") && matches {
                        if let Ok(request) = read_frame(&mut stream, MAX_REQUEST) {
                            if let Some(args) = request
                                .get("args")
                                .and_then(Value::as_array)
                                .and_then(|args| {
                                    args.iter()
                                        .map(|arg| arg.as_str().map(str::to_owned))
                                        .collect::<Option<Vec<_>>>()
                                })
                            {
                                dctx_cli::host::set_session_auto_clean(
                                    request
                                        .get("auto_clean")
                                        .and_then(Value::as_bool)
                                        .unwrap_or(false),
                                );
                                let reply = output_value(execute(&args));
                                if write_frame(&mut stream, &reply).is_ok() {
                                    // The acknowledgement bounds pipe shutdown without an unbounded
                                    // FlushFileBuffers call while ensuring the client consumed the reply.
                                    let _ = read_frame(&mut stream, MAX_REQUEST);
                                }
                            }
                        }
                    }
                    if op != Some("hello") || !matches {
                        let _ = read_frame(&mut stream, MAX_REQUEST);
                    }
                    if stopping {
                        return Ok(());
                    }
                }
                last = Instant::now();
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if last.elapsed() >= settings.idle {
                    return Ok(());
                }
                std::thread::sleep(POLL);
            }
            Err(error) => return Err(error),
        }
    }
}

fn output_value(result: Result<CommandOutput, Failure>) -> Value {
    let (stdout, failure) = match result {
        Ok(output) => (output.stdout, output.failure),
        Err(failure) => (String::new(), Some(failure)),
    };
    let failure = failure.map(
        |failure| json!({"code":failure.code, "message":failure.message, "hint":failure.hint}),
    );
    json!({"stdout":stdout, "failure":failure})
}

struct FrameBuffer(Vec<u8>);

impl Write for FrameBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) > MAX_REPLY {
            return Err(io::ErrorKind::InvalidData.into());
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn write_frame(output: &mut impl Write, value: &Value) -> io::Result<()> {
    // Bound the allocation while encoding, including JSON string escaping.
    let mut frame = FrameBuffer(Vec::new());
    serde_json::to_writer(&mut frame, value)?;
    let len = u32::try_from(frame.0.len()).map_err(|_| io::ErrorKind::InvalidData)?;
    output.write_all(&len.to_le_bytes())?;
    output.write_all(&frame.0)?;
    output.flush()
}

fn read_frame(input: &mut impl Read, max: usize) -> io::Result<Value> {
    let mut len = [0; 4];
    input.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    if len == 0 || len > max {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut bytes = vec![0; len];
    input.read_exact(&mut bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn executable_identity() -> io::Result<String> {
    // Fingerprint replacement without hashing a potentially large debug executable on every CLI
    // invocation. A new file, version, build, path, size or write timestamp forces a restart.
    let path = std::env::current_exe()?;
    let metadata = std::fs::metadata(&path)?;
    let modified = metadata
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let mut hash = dctx_hash::Sha256::new();
    hash.update(path.as_os_str().as_encoded_bytes())
        .map_err(io::Error::other)?;
    hash.update(&metadata.len().to_le_bytes())
        .map_err(io::Error::other)?;
    hash.update(&modified.to_le_bytes())
        .map_err(io::Error::other)?;
    Ok(format!(
        "daemon-v1-{}-{}-{}",
        env!("CARGO_PKG_VERSION"),
        env!("DCTX_GIT_SHA"),
        hash.finalize()
    ))
}

fn context_identity() -> io::Result<String> {
    let mut values: Vec<_> = std::env::vars_os()
        .filter(|(name, _)| name != "DCTX_DAEMON")
        .collect();
    values.sort();
    let mut hash = dctx_hash::Sha256::new();
    for value in std::iter::once(std::env::current_dir()?.into_os_string())
        .chain(values.into_iter().flat_map(|(name, value)| [name, value]))
    {
        let bytes = value.as_encoded_bytes();
        hash.update(&(bytes.len() as u64).to_le_bytes())
            .and_then(|()| hash.update(bytes))
            .map_err(io::Error::other)?;
    }
    Ok(hash.finalize().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_limits_reject_before_allocating_or_decoding_a_body() {
        assert!(read_frame(&mut &0u32.to_le_bytes()[..], MAX_REQUEST).is_err());
        assert!(read_frame(&mut &u32::MAX.to_le_bytes()[..], MAX_REQUEST).is_err());
        let mut frame = Vec::new();
        write_frame(&mut frame, &json!({"args":["map","find","é\n\"\\"]})).expect("write");
        assert_eq!(
            read_frame(&mut frame.as_slice(), MAX_REQUEST).expect("read")["args"][2],
            "é\n\"\\"
        );
        frame.pop();
        assert!(read_frame(&mut frame.as_slice(), MAX_REQUEST).is_err());
    }
}

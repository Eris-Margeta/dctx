// SPDX-License-Identifier: Apache-2.0
//! Ordered stdio transport. All operation output comes from the existing engine paths.

use std::io::{self, BufRead, Write};

use dctx_cli::exit::{CommandOutput, Failure};
use dctx_cli::rpc::{
    Error, Hello, Id, Outcome, Request, Response, ResultBody, Verb, PROTOCOL_VERSION,
};

/// Maximum request line size, excluding LF. Oversized lines are drained without allocating them.
const MAX_LINE_BYTES: usize = 1024 * 1024;
const HELP: &str = "Usage: dctx rpc\nNewline-delimited engine JSON over stdin/stdout. Send hello first.\nSee docs/protocol/README.md for v1 requests, ordering and cancellation.\n";

pub(crate) fn run(args: &[String]) -> Result<(), Failure> {
    match args {
        [] => {}
        [flag] if flag == "--help" || flag == "-h" => {
            print!("{HELP}");
            return Ok(());
        }
        _ => return Err(usage("usage: dctx rpc")),
    }
    let runtime = crate::context_cmd::Runtime::default();
    serve(io::stdin().lock(), io::stdout().lock(), &|request| {
        execute(request, &runtime)
    })
    .or_else(|error| {
        if error.kind() == io::ErrorKind::BrokenPipe {
            Ok(())
        } else {
            Err(Failure::index(
                error.to_string(),
                "restart the RPC engine; check stdin and stdout",
            ))
        }
    })
}

fn usage(message: impl Into<String>) -> Failure {
    Failure::usage(
        message,
        "see docs/protocol/README.md; send hello with an id first",
    )
}

fn response(id: Option<Id>, outcome: Outcome) -> Response {
    Response {
        id,
        engine_version: env!("CARGO_PKG_VERSION").to_string(),
        outcome,
    }
}

fn failed(id: Option<Id>, failure: Failure, output: String) -> Response {
    response(
        id,
        Outcome::Failure {
            error: Error {
                code: failure.code,
                message: failure.message,
                hint: failure.hint,
                output,
            },
        },
    )
}

/// Read one bounded line, consuming its entire frame even on overflow. A final line needs no LF.
fn read_frame(input: &mut impl BufRead, line: &mut Vec<u8>) -> io::Result<Option<bool>> {
    line.clear();
    let mut seen = false;
    let mut oversized = false;
    loop {
        let chunk = input.fill_buf()?;
        if chunk.is_empty() {
            return Ok(seen.then_some(oversized));
        }
        seen = true;
        let newline = chunk.iter().position(|byte| *byte == b'\n');
        let count = newline.unwrap_or(chunk.len());
        if !oversized {
            if line.len().saturating_add(count) > MAX_LINE_BYTES {
                oversized = true;
                line.clear();
            } else {
                line.extend_from_slice(&chunk[..count]);
            }
        }
        input.consume(count + usize::from(newline.is_some()));
        if newline.is_some() {
            return Ok(Some(oversized));
        }
    }
}

fn serve(
    mut input: impl BufRead,
    mut output: impl Write,
    exec: &dyn Fn(&Request) -> Result<CommandOutput, Failure>,
) -> io::Result<()> {
    let mut line = Vec::new();
    let mut ready = false;
    while let Some(oversized) = read_frame(&mut input, &mut line)? {
        let reply = if oversized {
            failed(
                None,
                usage("request line exceeds 1048576 bytes"),
                String::new(),
            )
        } else {
            handle(&line, &mut ready, exec)
        };
        serde_json::to_writer(&mut output, &reply)?;
        output.write_all(b"\n")?;
        output.flush()?;
    }
    Ok(())
}

fn handle(
    line: &[u8],
    ready: &mut bool,
    exec: &dyn Fn(&Request) -> Result<CommandOutput, Failure>,
) -> Response {
    // Decode the envelope token independently so unknown verbs and invalid args still echo a valid id.
    let id = serde_json::from_slice::<serde_json::Value>(line)
        .ok()
        .and_then(|value| value.get("id").cloned())
        .and_then(|id| serde_json::from_value(id).ok());
    let request: Request = match serde_json::from_slice(line) {
        Ok(request) => request,
        Err(error) => {
            return failed(
                id,
                usage(format!("invalid request: {error}")),
                String::new(),
            )
        }
    };
    let id = Some(request.id.clone());
    if request.verb == Verb::Hello {
        if !request.args.is_empty() {
            return failed(id, usage("hello takes no arguments"), String::new());
        }
        *ready = true;
        return response(
            id,
            Outcome::Success {
                result: ResultBody::Hello(Hello {
                    protocol_version: PROTOCOL_VERSION.to_string(),
                    verbs: Verb::ALL.to_vec(),
                }),
            },
        );
    }
    if !*ready {
        return failed(
            id,
            usage("hello is required before engine operations"),
            String::new(),
        );
    }
    match exec(&request) {
        Ok(CommandOutput {
            stdout,
            failure: Some(failure),
        }) => failed(id, failure, stdout),
        Ok(CommandOutput {
            stdout,
            failure: None,
        }) => response(
            id,
            Outcome::Success {
                result: ResultBody::Output { output: stdout },
            },
        ),
        Err(failure) => failed(id, failure, String::new()),
    }
}

fn execute(
    request: &Request,
    runtime: &crate::context_cmd::Runtime,
) -> Result<CommandOutput, Failure> {
    let mut args = request.args.clone();
    // JSON is the protocol's output format. No other argument is changed or retried.
    let unique_format = matches!(
        request.verb,
        Verb::Languages | Verb::CacheStatus | Verb::Context
    );
    let explicit_json = if request.verb == Verb::Context {
        crate::context_cmd::has_flag(&args, "--json")
    } else {
        args.iter().any(|arg| arg == "--json")
    };
    if !unique_format || !explicit_json {
        args.insert(0, "--json".to_string());
    }
    match request.verb {
        Verb::Hello => Err(usage("hello is handled by the session")),
        Verb::Context if !crate::context_cmd::has_flag(&args, "--for") => runtime.output(&args),
        Verb::Pack | Verb::Context => {
            args.insert(0, "pack".to_string());
            if request.verb == Verb::Context && !crate::pack_for::requested(&args[1..]) {
                return Err(usage("context requires --for TEXT"));
            }
            if crate::pack_for::requested(&args[1..]) {
                crate::pack_for::output(&args)
            } else {
                dctx_cli::pack::output(&args)
            }
        }
        Verb::Languages => dctx_cli::languages::output(&args),
        Verb::CacheStatus => {
            args.splice(0..0, ["cache".to_string(), "status".to_string()]);
            dctx_cli::cache::status_output(&args)
        }
        verb => {
            args.insert(0, verb.as_str().to_string());
            crate::map::output(&args)
        }
    }
}

#[cfg(test)]
mod tests;

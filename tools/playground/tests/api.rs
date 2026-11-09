// SPDX-License-Identifier: Apache-2.0
//! End-to-end tests of the HTTP API: the real dctx-play binary, a stand-in `dctx` script.
//! The real binary is exercised by tools/playground/smoke.sh.

use std::io::{BufRead, Read, Write};
use std::net::TcpStream;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const FAKE_DCTX: &str = r#"#!/bin/sh
if [ "$1" = "--version" ]; then echo "dctx 1.1.0 (abc1234)"; exit 0; fi
if [ "$1" = "mcp" ]; then while IFS= read -r line; do printf '%s\n' "$line"; done; exit 0; fi
if [ "$1" = "map" ] && [ "$2" = "find" ] && [ "$3" = "slow" ]; then exec /bin/sleep 10; fi
if [ "$1" = "map" ] && [ "$2" = "find" ] && [ "$3" = "pause" ]; then /bin/sleep 1; fi
printf 'place=%s\n' "$(/usr/bin/basename "$PWD")"
for a in "$@"; do printf 'arg=%s\n' "$a"; done
[ -f src/a.rs ] && /bin/cat src/a.rs
exit 0
"#;

struct Server {
    child: Child,
    addr: String,
    root: PathBuf,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// The stand-in binary is written once, before any test spawns a process: writing an executable
/// while other threads fork can make its exec fail with ETXTBSY ("Text file busy").
fn fake_dctx() -> &'static PathBuf {
    static FAKE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    FAKE.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("dctx-play-api-fake-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("dctx");
        std::fs::write(&fake, FAKE_DCTX).unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        fake
    })
}

fn start(extra: &[&str]) -> Server {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("dctx-play-api-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("samples/kiosk/src")).unwrap();
    std::fs::write(root.join("samples/kiosk/src/main.rs"), "fn main() {}\n").unwrap();
    std::fs::create_dir_all(root.join("samples/other")).unwrap();
    std::fs::write(root.join("samples/other/a.md"), "# a\n").unwrap();
    std::fs::write(root.join("samples/index.tsv"), "kiosk\tThe kiosk sample\nother\tOther\n").unwrap();
    let fake = fake_dctx();
    // The server binds port 0 itself and reports the bound address. Reserving a port here and
    // releasing it raced with parallel tests, which then talked to each other's servers.
    let mut args = vec![
        "--listen".to_string(), "127.0.0.1:0".into(),
        "--samples".into(), root.join("samples").display().to_string(),
        "--dctx".into(), fake.display().to_string(),
        "--work".into(), root.join("work").display().to_string(),
        "--nproc".into(), "0".into(),
        "--timeout-ms".into(), "500".into(),
    ];
    args.extend(extra.iter().map(|s| s.to_string()));
    let mut child = Command::new(env!("CARGO_BIN_EXE_dctx-play")).args(&args).stderr(Stdio::piped()).spawn().unwrap();
    let mut stderr = std::io::BufReader::new(child.stderr.take().unwrap());
    let mut line = String::new();
    let addr = loop {
        line.clear();
        assert!(stderr.read_line(&mut line).unwrap() > 0, "server exited before listening");
        if let Some(rest) = line.strip_prefix("dctx-play: listening on ") {
            break rest.split(' ').next().unwrap().to_string();
        }
    };
    // Keep draining so the server never blocks on a full stderr pipe.
    std::thread::spawn(move || std::io::copy(&mut stderr, &mut std::io::sink()));
    Server { child, addr, root }
}

/// Sends raw bytes, returns (status, headers, body).
fn raw(addr: &str, request: &[u8]) -> (u16, String, String) {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
    stream.write_all(request).unwrap();
    let mut out = Vec::new();
    let _ = stream.read_to_end(&mut out);
    let text = String::from_utf8_lossy(&out).to_string();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head.split(' ').nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    (status, head.to_string(), body.to_string())
}

fn get(addr: &str, path: &str) -> (u16, String, String) {
    raw(addr, format!("GET {path} HTTP/1.1\r\nHost: t\r\n\r\n").as_bytes())
}

fn post(addr: &str, path: &str, kind: &str, body: &str) -> (u16, String, String) {
    raw(addr, format!("POST {path} HTTP/1.1\r\nHost: t\r\nContent-Type: {kind}\r\nContent-Length: {}\r\n\r\n{body}", body.len()).as_bytes())
}

fn run(addr: &str, body: &str) -> (u16, String, String) {
    post(addr, "/api/v1/run", "application/json", body)
}

#[test]
fn health_reports_version_sha_and_limits() {
    let s = start(&[]);
    let (status, head, body) = get(&s.addr, "/api/v1/health");
    assert_eq!(status, 200);
    assert!(head.contains("Cache-Control: public, max-age=10"));
    assert!(body.starts_with(r#"{"ok":true,"dctx":"1.1.0","sha":"abc1234","samples":2,"limits":{"#), "{body}");
}

#[test]
fn samples_are_listed_in_index_order() {
    let s = start(&[]);
    let (status, _, body) = get(&s.addr, "/api/v1/samples");
    assert_eq!(status, 200);
    assert_eq!(body, r#"[{"id":"kiosk","title":"The kiosk sample","files":1,"bytes":13},{"id":"other","title":"Other","files":1,"bytes":4}]"#);
}

#[test]
fn openapi_and_form_css_are_served() {
    let s = start(&[]);
    let (status, _, body) = get(&s.addr, "/api/v1/openapi.json");
    assert_eq!(status, 200);
    assert!(body.contains("\"openapi\": \"3.1.0\""));
    let (status, head, _) = get(&s.addr, "/api/v1/form.css");
    assert_eq!(status, 200);
    assert!(head.contains("text/css"));
}

#[test]
fn run_on_a_sample_uses_the_sample_name_as_the_place() {
    let s = start(&[]);
    let (status, _, body) = run(&s.addr, r#"{"cmd":["map","find","greet"],"sample":"kiosk"}"#);
    assert_eq!(status, 200, "{body}");
    assert!(body.contains(r#""stdout":"place=kiosk\narg=map\narg=find\narg=greet\n""#), "{body}");
    assert!(body.contains(r#""exit":0"#));
    assert!(body.contains(r#""est_tokens":"#));
}

#[test]
fn run_on_pasted_files() {
    let s = start(&[]);
    let (status, _, body) = run(&s.addr, r#"{"cmd":["map","find","a"],"files":[{"path":"src/a.rs","content":"fn a() {}\n"}]}"#);
    assert_eq!(status, 200, "{body}");
    assert!(body.contains(r#"place=project\n"#) && body.contains(r#"fn a() {}\n"#), "{body}");
}

#[test]
fn grammar_refusals_are_400_with_a_reason() {
    let s = start(&["--burst", "100", "--rate", "6000"]);
    for cmd in [
        r#"["cache","purge","--all","--yes"]"#,
        r#"["map","find","greet","--place","/"]"#,
        r#"["pack",".","--output","x"]"#,
        r#"["map","read","/etc/passwd"]"#,
        r#"["map","read","m:../x"]"#,
        r#"["map","neighbors","s:a","--depth","9"]"#,
        r#"["template","build","x"]"#,
        r#"[".","-d"]"#,
        r#"["map","export","--output","g.mmd"]"#,
        r#"[]"#,
    ] {
        let (status, _, body) = run(&s.addr, &format!(r#"{{"cmd":{cmd}}}"#));
        assert_eq!(status, 400, "{cmd}: {body}");
        assert!(body.contains(r#""reason":"#), "{body}");
    }
}

#[test]
fn malformed_bodies_are_refused() {
    let s = start(&[]);
    for body in [
        "not json",
        "[]",
        r#"{"cmd":"map find"}"#,
        r#"{"cmd":["map","find","x"],"extra":1}"#,
        r#"{"cmd":["map","find","x"],"sample":"nope"}"#,
        r#"{"cmd":["map","find","x"],"sample":"kiosk","files":[]}"#,
        r#"{"cmd":["map","find","x"],"files":[{"path":"../a","content":"x"}]}"#,
        r#"{"cmd":["map","find","x"],"files":[{"path":"a","content":"x","mode":"755"}]}"#,
        r#"{"cmd":[[[[[[[[["deep"]]]]]]]]]}"#,
    ] {
        let (status, _, reply) = run(&s.addr, body);
        assert_eq!(status, 400, "{body}: {reply}");
    }
}

#[test]
fn wrong_content_type_is_415() {
    let s = start(&[]);
    let (status, _, _) = post(&s.addr, "/api/v1/run", "text/plain", r#"{"cmd":["languages"]}"#);
    assert_eq!(status, 415);
}

#[test]
fn missing_length_is_411_and_oversize_is_413() {
    let s = start(&[]);
    let (status, _, _) = raw(&s.addr, b"POST /api/v1/run HTTP/1.1\r\nHost: t\r\nContent-Type: application/json\r\n\r\n");
    assert_eq!(status, 411);
    let (status, _, _) = raw(&s.addr, b"POST /api/v1/run HTTP/1.1\r\nHost: t\r\nContent-Type: application/json\r\nContent-Length: 600000\r\n\r\n");
    assert_eq!(status, 413);
    let (status, _, _) = raw(&s.addr, b"POST /api/v1/run HTTP/1.1\r\nHost: t\r\nTransfer-Encoding: chunked\r\n\r\n");
    assert_eq!(status, 411);
}

#[test]
fn unknown_paths_and_methods() {
    let s = start(&[]);
    assert_eq!(get(&s.addr, "/api/v1/nope").0, 404);
    assert_eq!(get(&s.addr, "/api/v1/run").0, 405);
    assert_eq!(post(&s.addr, "/api/v1/health", "application/json", "{}").0, 405);
    assert_eq!(raw(&s.addr, b"BREW / HTTP/1.1\r\n\r\n").0, 404);
    assert_eq!(raw(&s.addr, b"GET / HTTP/9\r\n\r\n").0, 400);
}

#[test]
fn per_client_rate_limit_is_429_with_retry_after() {
    let s = start(&["--burst", "2", "--rate", "1"]);
    for i in 0..2 {
        assert_eq!(run(&s.addr, &format!(r#"{{"cmd":["map","find","q{i}"]}}"#)).0, 200);
    }
    let (status, head, _) = run(&s.addr, r#"{"cmd":["map","find","q9"]}"#);
    assert_eq!(status, 429);
    assert!(head.contains("Retry-After: "));
    // Another visitor behind the proxy has its own bucket.
    let body = r#"{"cmd":["map","find","q9"]}"#;
    let other = format!("POST /api/v1/run HTTP/1.1\r\nHost: t\r\nX-Forwarded-For: 203.0.113.7, 10.0.0.2\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len());
    assert_eq!(raw(&s.addr, other.as_bytes()).0, 200);
}

#[test]
fn a_spoofed_first_forwarded_entry_does_not_get_a_new_bucket() {
    let s = start(&["--burst", "2", "--rate", "1"]);
    let send = |i: usize, header: &str| {
        let body = format!(r#"{{"cmd":["map","find","s{i}"]}}"#);
        let req = format!("POST /api/v1/run HTTP/1.1\r\nHost: t\r\n{header}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len());
        raw(&s.addr, req.as_bytes()).0
    };
    // The proxy appends the visitor as the last entry; a new first entry each time changes nothing.
    let codes: Vec<u16> = (0..3).map(|i| send(i, &format!("X-Forwarded-For: 192.0.2.{i}, 198.51.100.9"))).collect();
    assert_eq!(codes, [200, 200, 429]);
    // X-Real-IP (set by the proxy) wins over any X-Forwarded-For value.
    let codes: Vec<u16> = (3..6)
        .map(|i| send(i, &format!("X-Real-IP: 198.51.100.10\r\nX-Forwarded-For: 192.0.2.{i}")))
        .collect();
    assert_eq!(codes, [200, 200, 429]);
}

#[test]
fn identical_requests_come_from_the_cache_without_using_the_rate() {
    let s = start(&["--burst", "1", "--rate", "1"]);
    let body = r#"{"cmd":["map","find","greet"]}"#;
    let (first, _, a) = run(&s.addr, body);
    let (second, head, b) = run(&s.addr, body);
    assert_eq!((first, second), (200, 200));
    assert!(head.contains("X-Cache: hit"));
    assert_eq!(a, b);
}

#[test]
fn a_full_queue_is_503() {
    let s = start(&["--burst", "100", "--rate", "6000", "--concurrency", "1", "--threads", "1", "--queue", "1", "--timeout-ms", "3000"]);
    let addr = s.addr.clone();
    let handles: Vec<_> = (0..6)
        .map(|i| {
            let addr = addr.clone();
            std::thread::spawn(move || run(&addr, &format!(r#"{{"cmd":["map","find","pause","--limit","{}"]}}"#, i + 1)).0)
        })
        .collect();
    let codes: Vec<u16> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert!(codes.contains(&503), "{codes:?}");
    assert!(codes.contains(&200), "{codes:?}");
}

#[test]
fn a_slow_run_is_killed_at_the_wall_clock_limit() {
    let s = start(&[]);
    let started = Instant::now();
    let (status, _, body) = run(&s.addr, r#"{"cmd":["map","find","slow"]}"#);
    assert_eq!(status, 200);
    assert!(body.contains(r#""exit":null,"killed":"timeout""#), "{body}");
    assert!(started.elapsed() < Duration::from_secs(5));
    let (_, _, metrics) = get(&s.addr, "/api/v1/metrics.json");
    assert!(metrics.contains(r#""timeouts_total":1"#), "{metrics}");
}

#[test]
fn mcp_feeds_requests_on_stdin_and_returns_responses() {
    let s = start(&[]);
    let body = r#"{"requests":[{"jsonrpc":"2.0","id":1,"method":"tools/list"},{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"find","arguments":{"query":"greet"}}}]}"#;
    let (status, _, reply) = post(&s.addr, "/api/v1/mcp", "application/json", body);
    assert_eq!(status, 200, "{reply}");
    assert!(reply.starts_with(r#"{"responses":[{"jsonrpc":"2.0","id":1,"method":"tools/list"},"#), "{reply}");
}

#[test]
fn mcp_refuses_unknown_methods_tools_and_too_many_requests() {
    let s = start(&["--burst", "100", "--rate", "6000"]);
    for body in [
        r#"{"requests":[]}"#,
        r#"{"requests":[{"jsonrpc":"2.0","id":1,"method":"shutdown"}]}"#,
        r#"{"requests":[{"jsonrpc":"1.0","id":1,"method":"tools/list"}]}"#,
        r#"{"requests":[{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"rm"}}]}"#,
        r#"{"requests":[1,2,3,4,5]}"#,
    ] {
        assert_eq!(post(&s.addr, "/api/v1/mcp", "application/json", body).0, 400, "{body}");
    }
}

#[test]
fn the_form_works_without_javascript() {
    let s = start(&[]);
    let (status, head, body) = post(&s.addr, "/api/v1/run-form", "application/x-www-form-urlencoded", "cmd=map+find+%27a+b%27&source=sample&sample=kiosk&files=");
    assert_eq!(status, 200);
    assert!(head.contains("text/html") && head.contains("Content-Security-Policy: default-src 'none'"));
    assert!(body.contains("<h1>dctx map find &#39;a b&#39;</h1>"), "{body}");
    assert!(body.contains("arg=a b"), "{body}");
    let files = "cmd=map+find+a&source=files&files=---+src%2Fa.rs+---%0Afn+a%28%29+%7B%7D%0A";
    let (status, _, body) = post(&s.addr, "/api/v1/run-form", "application/x-www-form-urlencoded", files);
    assert_eq!(status, 200);
    assert!(body.contains("place=project") && body.contains("fn a() {}"), "{body}");
    let (status, _, body) = post(&s.addr, "/api/v1/run-form", "application/x-www-form-urlencoded", "cmd=cache+purge+--all");
    assert_eq!(status, 400);
    assert!(body.contains("not offered"), "{body}");
}

#[test]
fn head_requests_carry_no_body() {
    let s = start(&[]);
    let (status, head, body) = raw(&s.addr, b"HEAD /api/v1/health HTTP/1.1\r\nHost: t\r\n\r\n");
    assert_eq!(status, 200);
    assert!(head.contains("Content-Length: 0"));
    assert!(body.is_empty());
}

#[test]
fn health_probe_mode_exits_zero_when_up() {
    let s = start(&[]);
    let status = Command::new(env!("CARGO_BIN_EXE_dctx-play")).args(["--health", &s.addr]).status().unwrap();
    assert!(status.success());
    drop(s);
    let status = Command::new(env!("CARGO_BIN_EXE_dctx-play")).args(["--health", "127.0.0.1:9"]).status().unwrap();
    assert!(!status.success());
}

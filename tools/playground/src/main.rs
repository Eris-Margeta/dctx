// SPDX-License-Identifier: Apache-2.0
//! dctx-play: the dctx website playground.
//!
//! A blocking HTTP/1.1 server (standard library only) that runs the real `dctx` binary on a
//! bundled sample or on pasted text files, through a whitelisted command grammar, with
//! per-run resource limits. Nothing is persisted. See tools/playground/README.md.

mod form;
mod grammar;
mod http;
#[allow(dead_code)]
#[path = "../../../crates/dctx-cli/src/mcp/json.rs"]
mod json;
mod limits;
mod sandbox;
mod sha256;

use std::io::Write as _;
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{sync_channel, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use http::{ReadError, Request, Response};
use json::Json;

const OPENAPI: &str = include_str!("../openapi.json");
const FORM_CSS: &str = include_str!("form.css");
const MAX_RESPONSE: usize = 300 * 1024;
const MCP_METHODS: [&str; 5] = ["initialize", "notifications/initialized", "tools/list", "tools/call", "ping"];
const USAGE: &str = "Usage: dctx-play [--listen ADDR] [--samples DIR] [--dctx PATH] [--work DIR]
                 [--rate N] [--burst N] [--concurrency N] [--queue N] [--threads N]
                 [--timeout-ms N] [--nproc N]
       dctx-play --health [ADDR]
       dctx-play --version

Defaults: --listen 0.0.0.0:8080 --samples /samples --dctx /usr/local/bin/dctx --work /run/play
          --rate 20 (per minute per client) --burst 5 --concurrency 2 --queue 8 --threads 4
          --timeout-ms 3000 --nproc 16 (0 = leave RLIMIT_NPROC alone; it counts every process of the user)
";

#[derive(Clone, Debug)]
struct Config {
    listen: String,
    samples: PathBuf,
    dctx: PathBuf,
    work: PathBuf,
    rate: u32,
    burst: u32,
    concurrency: usize,
    queue: usize,
    threads: usize,
    limits: sandbox::Limits,
}

fn parse_args(args: &[String]) -> Result<Config, String> {
    let mut cfg = Config {
        listen: "0.0.0.0:8080".into(),
        samples: "/samples".into(),
        dctx: "/usr/local/bin/dctx".into(),
        work: "/run/play".into(),
        rate: 20,
        burst: 5,
        concurrency: 2,
        queue: 8,
        threads: 4,
        limits: sandbox::Limits::default(),
    };
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        let value = args.get(i + 1).ok_or_else(|| format!("{flag} needs a value\n\n{USAGE}"))?;
        let number = || value.parse::<u64>().map_err(|_| format!("{flag} takes a number"));
        match flag {
            "--listen" => cfg.listen = value.clone(),
            "--samples" => cfg.samples = value.into(),
            "--dctx" => cfg.dctx = value.into(),
            "--work" => cfg.work = value.into(),
            "--rate" => cfg.rate = number()? as u32,
            "--burst" => cfg.burst = number()? as u32,
            "--concurrency" => cfg.concurrency = number()?.max(1) as usize,
            "--queue" => cfg.queue = number()? as usize,
            "--threads" => cfg.threads = number()?.max(1) as usize,
            "--timeout-ms" => cfg.limits.wall = Duration::from_millis(number()?.max(1)),
            "--nproc" => cfg.limits.processes = number()?,
            _ => return Err(format!("unknown option {flag}\n\n{USAGE}")),
        }
        i += 2;
    }
    Ok(cfg)
}

struct Sample {
    id: String,
    title: String,
    dir: PathBuf,
    files: u64,
    bytes: u64,
}

fn walk(dir: &Path, files: &mut u64, bytes: &mut u64) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => walk(&entry.path(), files, bytes),
                Ok(kind) if kind.is_file() => {
                    *files += 1;
                    *bytes += entry.metadata().map(|m| m.len()).unwrap_or(0);
                }
                _ => {}
            }
        }
    }
}

/// Every subdirectory of `root` is a sample; `root/index.tsv` (`id<TAB>title`) gives titles and order.
fn load_samples(root: &Path) -> Result<Vec<Sample>, String> {
    let index = std::fs::read_to_string(root.join("index.tsv")).unwrap_or_default();
    let mut titled: Vec<(String, String)> =
        index.lines().filter_map(|l| l.split_once('\t')).map(|(a, b)| (a.to_string(), b.to_string())).collect();
    let mut ids: Vec<String> = std::fs::read_dir(root)
        .map_err(|e| format!("cannot read samples directory {}: {e}", root.display()))?
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|id| grammar::check_path(id).is_ok() && !id.contains('/'))
        .collect();
    ids.sort();
    for id in &ids {
        if !titled.iter().any(|(t, _)| t == id) {
            titled.push((id.clone(), id.clone()));
        }
    }
    let mut samples = Vec::new();
    for (id, title) in titled {
        if !ids.contains(&id) {
            continue;
        }
        let dir = root.join(&id);
        let (mut files, mut bytes) = (0, 0);
        walk(&dir, &mut files, &mut bytes);
        samples.push(Sample { id, title, dir, files, bytes });
    }
    if samples.is_empty() {
        return Err(format!("no samples under {}", root.display()));
    }
    Ok(samples)
}

struct App {
    cfg: Config,
    samples: Vec<Sample>,
    health: String,
    samples_json: String,
    buckets: limits::Buckets,
    gate: limits::Gate,
    cache: limits::Cache<Response>,
    metrics: limits::Metrics,
}

fn num(n: u64) -> Json {
    Json::Num(n.to_string())
}

fn error(status: u16, reason: &str) -> Response {
    Response::json(status, Json::obj(vec![("error", Json::str(http::reason(status))), ("reason", Json::str(reason))]).to_line())
}

/// The depth of a JSON value (a scalar is 1).
fn depth(value: &Json) -> usize {
    match value {
        Json::Arr(items) => 1 + items.iter().map(depth).max().unwrap_or(0),
        Json::Obj(members) => 1 + members.iter().map(|(_, v)| depth(v)).max().unwrap_or(0),
        _ => 1,
    }
}

fn client(req: &Request, peer: &str) -> String {
    // Traefik (and the nginx in front of this service) are the only peers; the first
    // X-Forwarded-For entry is the visitor. Without the header the peer address is used.
    req.header("x-forwarded-for")
        .and_then(|v| v.split(',').next())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty() && v.len() <= 64)
        .unwrap_or_else(|| peer.to_string())
}

enum Place {
    Sample(usize),
    Files(Vec<grammar::File>),
}

impl App {
    fn new(cfg: Config) -> Result<App, String> {
        let samples = load_samples(&cfg.samples)?;
        let out = std::process::Command::new(&cfg.dctx)
            .arg("--version")
            .env_clear()
            .output()
            .map_err(|e| format!("cannot run {}: {e}", cfg.dctx.display()))?;
        let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let mut words = text.split_whitespace();
        let (name, version, sha) = (words.next(), words.next(), words.next());
        if name != Some("dctx") || version.is_none() {
            return Err(format!("{} --version printed {text:?}", cfg.dctx.display()));
        }
        let sha = sha.unwrap_or("(unknown)").trim_matches(|c| c == '(' || c == ')').to_string();
        std::fs::create_dir_all(&cfg.work).map_err(|e| format!("cannot create {}: {e}", cfg.work.display()))?;
        let l = &cfg.limits;
        let limits = Json::obj(vec![
            ("requests_per_minute", num(u64::from(cfg.rate))),
            ("burst", num(u64::from(cfg.burst))),
            ("concurrent_runs", num(cfg.concurrency as u64)),
            ("queue", num(cfg.queue as u64)),
            ("wall_ms", num(l.wall.as_millis() as u64)),
            ("cpu_seconds", num(l.cpu_seconds)),
            ("memory_bytes", num(l.address_space)),
            ("max_files", num(grammar::MAX_FILES as u64)),
            ("max_file_bytes", num(grammar::MAX_FILE_BYTES as u64)),
            ("max_total_bytes", num(grammar::MAX_TOTAL_BYTES as u64)),
            ("max_body_bytes", num(http::MAX_BODY as u64)),
            ("max_output_bytes", num(l.output_cap as u64)),
        ]);
        let health = Json::obj(vec![
            ("ok", Json::Bool(true)),
            ("dctx", Json::str(version.unwrap_or_default())),
            ("sha", Json::str(&sha)),
            ("samples", num(samples.len() as u64)),
            ("limits", limits),
        ])
        .to_line();
        let samples_json = Json::Arr(
            samples
                .iter()
                .map(|s| {
                    Json::obj(vec![("id", Json::str(&s.id)), ("title", Json::str(&s.title)), ("files", num(s.files)), ("bytes", num(s.bytes))])
                })
                .collect(),
        )
        .to_line();
        Ok(App {
            buckets: limits::Buckets::new(cfg.rate, cfg.burst),
            gate: limits::Gate::new(cfg.concurrency, cfg.threads),
            cache: limits::Cache::new(64, Duration::from_secs(60)),
            metrics: limits::Metrics::default(),
            samples,
            health,
            samples_json,
            cfg,
        })
    }

    fn handle(&self, req: &Request, peer: &str) -> Response {
        let route = (req.method.as_str(), req.path.as_str());
        match route {
            ("GET" | "HEAD", "/api/v1/health") => {
                Response::json(200, self.health.clone()).with("Cache-Control", "public, max-age=10")
            }
            ("GET" | "HEAD", "/api/v1/samples") => {
                Response::json(200, self.samples_json.clone()).with("Cache-Control", "public, max-age=60")
            }
            ("GET" | "HEAD", "/api/v1/openapi.json") => {
                Response::json(200, OPENAPI.to_string()).with("Cache-Control", "public, max-age=300")
            }
            ("GET" | "HEAD", "/api/v1/form.css") => {
                Response::new(200, "text/css; charset=utf-8", FORM_CSS).with("Cache-Control", "public, max-age=3600")
            }
            ("GET" | "HEAD", "/api/v1/metrics.json") => self.metrics_json(),
            ("POST", "/api/v1/run") => self.post(req, peer, "run"),
            ("POST", "/api/v1/mcp") => self.post(req, peer, "mcp"),
            ("POST", "/api/v1/run-form") => self.post(req, peer, "form"),
            (_, "/api/v1/health" | "/api/v1/samples" | "/api/v1/openapi.json" | "/api/v1/metrics.json" | "/api/v1/form.css") => {
                error(405, "use GET").with("Allow", "GET, HEAD")
            }
            (_, "/api/v1/run" | "/api/v1/mcp" | "/api/v1/run-form") => error(405, "use POST").with("Allow", "POST"),
            _ => error(404, "no such endpoint; see /api/v1/openapi.json"),
        }
    }

    fn metrics_json(&self) -> Response {
        let (runs, hits, timeouts, rejected, p50) = self.metrics.snapshot();
        let (running, waiting) = self.gate.load();
        let body = Json::obj(vec![
            ("runs_total", num(runs)),
            ("cache_hits_total", num(hits)),
            ("timeouts_total", num(timeouts)),
            ("rejected_total", Json::Obj(rejected.into_iter().map(|(k, v)| (k, num(v))).collect())),
            ("p50_ms", num(p50)),
            ("running", num(running as u64)),
            ("waiting", num(waiting as u64)),
            ("cached", num(self.cache.len() as u64)),
        ]);
        Response::json(200, body.to_line())
    }

    fn reject(&self, kind: &str, status: u16, reason: &str) -> Response {
        self.metrics.reject(match status {
            429 => "rate",
            503 => "busy",
            413 => "size",
            _ => "request",
        });
        if kind == "form" {
            let html = form::page("Not run", reason, &[]);
            Response::new(status, "text/html; charset=utf-8", html).with("Content-Security-Policy", FORM_CSP)
        } else {
            error(status, reason)
        }
    }

    fn post(&self, req: &Request, peer: &str, kind: &str) -> Response {
        let content_type = req.header("content-type").unwrap_or("").to_ascii_lowercase();
        let wanted = if kind == "form" { "application/x-www-form-urlencoded" } else { "application/json" };
        if !content_type.starts_with(wanted) {
            return self.reject(kind, 415, &format!("send {wanted}"));
        }
        let Ok(body) = std::str::from_utf8(&req.body) else {
            return self.reject(kind, 400, "the body is not UTF-8");
        };
        let parsed = match kind {
            "run" => self.parse_run(body),
            "mcp" => self.parse_mcp(body),
            _ => self.parse_form(body),
        };
        let (argv, place, stdin, title) = match parsed {
            Ok(parts) => parts,
            Err(reason) => return self.reject(kind, 400, &reason),
        };
        let key = sha256::hex(format!("{kind}\0{body}").as_bytes());
        let now = Instant::now();
        if let Some(hit) = self.cache.get(&key, now) {
            self.metrics.hit();
            return hit.with("X-Cache", "hit");
        }
        if let Err(wait) = self.buckets.take(&client(req, peer), now) {
            return self.reject(kind, 429, "too many runs from one address; wait a moment").with("Retry-After", wait.to_string());
        }
        let Some(slot) = self.gate.enter() else {
            return self.reject(kind, 503, "the playground is busy; try again in a few seconds").with("Retry-After", "2");
        };
        let source = match &place {
            Place::Sample(index) => sandbox::Source::Sample { id: &self.samples[*index].id, dir: &self.samples[*index].dir },
            Place::Files(files) => sandbox::Source::Files(files),
        };
        let outcome = sandbox::run(&self.cfg.dctx, &self.cfg.work, source, &argv, stdin.as_deref(), &self.cfg.limits);
        drop(slot);
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(e) => {
                eprintln!("dctx-play: run failed: {e}");
                self.metrics.reject("internal");
                return error(500, "the run could not be started");
            }
        };
        self.metrics.run(outcome.ms, outcome.killed == Some("timeout"));
        let response = match kind {
            "run" => run_response(outcome),
            "mcp" => mcp_response(outcome),
            _ => form_response(&title, outcome),
        };
        self.cache.put(&key, response.clone(), Instant::now());
        response
    }

    fn place(&self, sample: Option<&Json>, files: Option<&Json>) -> Result<Place, String> {
        match (sample, files) {
            (Some(_), Some(_)) => Err("send either sample or files, not both".into()),
            (_, Some(Json::Arr(items))) => {
                let mut out = Vec::new();
                for item in items {
                    let path = item.get("path").and_then(Json::as_str).ok_or("each file needs a path string")?;
                    let content = item.get("content").and_then(Json::as_str).ok_or("each file needs a content string")?;
                    if let Json::Obj(members) = item {
                        if members.iter().any(|(k, _)| k != "path" && k != "content") {
                            return Err("a file has only path and content".into());
                        }
                    }
                    out.push(grammar::File { path: path.to_string(), content: content.to_string() });
                }
                grammar::check_files(&out)?;
                Ok(Place::Files(out))
            }
            (_, Some(_)) => Err("files is a list of {path, content}".into()),
            (sample, None) => {
                let id = match sample {
                    None => "kiosk",
                    Some(value) => value.as_str().ok_or("sample is a string")?,
                };
                self.samples.iter().position(|s| s.id == id).map(Place::Sample).ok_or_else(|| format!("no sample {id:?}"))
            }
        }
    }

    fn parse_json(body: &str, keys: &[&str]) -> Result<Json, String> {
        let value = json::parse(body).map_err(|e| format!("invalid JSON: {e}"))?;
        if depth(&value) > 8 {
            return Err("JSON nested deeper than 8 levels".into());
        }
        match &value {
            Json::Obj(members) => {
                if let Some((key, _)) = members.iter().find(|(k, _)| !keys.contains(&k.as_str())) {
                    return Err(format!("unknown field {key:?}"));
                }
            }
            _ => return Err("the body is a JSON object".into()),
        }
        Ok(value)
    }

    #[allow(clippy::type_complexity)]
    fn parse_run(&self, body: &str) -> Result<(Vec<String>, Place, Option<String>, String), String> {
        let value = Self::parse_json(body, &["cmd", "sample", "files"])?;
        let argv: Vec<String> = match value.get("cmd") {
            Some(Json::Arr(items)) => items.iter().map(|i| i.as_str().map(str::to_string).ok_or("cmd is a list of strings")).collect::<Result<_, _>>()?,
            _ => return Err("cmd is a list of strings, for example [\"map\",\"find\",\"greet\"]".into()),
        };
        grammar::check(&argv)?;
        let place = self.place(value.get("sample"), value.get("files"))?;
        Ok((argv, place, None, String::new()))
    }

    #[allow(clippy::type_complexity)]
    fn parse_mcp(&self, body: &str) -> Result<(Vec<String>, Place, Option<String>, String), String> {
        let value = Self::parse_json(body, &["requests", "sample", "files"])?;
        let Some(Json::Arr(requests)) = value.get("requests") else {
            return Err("requests is a list of JSON-RPC messages".into());
        };
        if requests.is_empty() || requests.len() > 4 {
            return Err("send 1 to 4 requests".into());
        }
        let mut stdin = String::new();
        for request in requests {
            if request.get("jsonrpc").and_then(Json::as_str) != Some("2.0") {
                return Err("each request has \"jsonrpc\":\"2.0\"".into());
            }
            let method = request.get("method").and_then(Json::as_str).ok_or("each request has a method")?;
            if !MCP_METHODS.contains(&method) {
                return Err(format!("method {method:?} is not offered; use {}", MCP_METHODS.join(", ")));
            }
            if method == "tools/call" {
                let params = request.get("params").ok_or("tools/call needs params")?;
                let name = params.get("name").and_then(Json::as_str).ok_or("tools/call needs params.name")?;
                if !grammar::VERBS.contains(&name) {
                    return Err(format!("no tool {name:?}"));
                }
            }
            let line = request.to_line();
            if line.len() > 4096 {
                return Err("a request is at most 4 KiB".into());
            }
            stdin.push_str(&line);
            stdin.push('\n');
        }
        let place = self.place(value.get("sample"), value.get("files"))?;
        Ok((vec!["mcp".into(), ".".into()], place, Some(stdin), String::new()))
    }

    #[allow(clippy::type_complexity)]
    fn parse_form(&self, body: &str) -> Result<(Vec<String>, Place, Option<String>, String), String> {
        let fields = form::parse_urlencoded(body)?;
        let field = |name: &str| fields.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str());
        let line = field("cmd").unwrap_or("").trim().to_string();
        let argv = grammar::split(&line)?;
        grammar::check(&argv)?;
        let files_text = field("files").unwrap_or("");
        let use_files = field("source") == Some("files") || (field("source").is_none() && !files_text.trim().is_empty());
        let place = if use_files {
            let files = form::parse_files(files_text)?;
            grammar::check_files(&files)?;
            Place::Files(files)
        } else {
            let id = field("sample").unwrap_or("kiosk");
            Place::Sample(self.samples.iter().position(|s| s.id == id).ok_or_else(|| format!("no sample {id:?}"))?)
        };
        let title = format!("dctx {}", argv.iter().map(|a| shell_quote(a)).collect::<Vec<_>>().join(" "));
        Ok((argv, place, None, title))
    }
}

const FORM_CSP: &str = "default-src 'none'; style-src 'self'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'";

fn shell_quote(arg: &str) -> String {
    if !arg.is_empty() && arg.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_./:=,".contains(&b)) {
        arg.to_string()
    } else {
        format!("'{}'", arg.replace('\'', "'\\''"))
    }
}

fn est_tokens(bytes: u64) -> u64 {
    (2 * bytes).div_ceil(7)
}

fn exit_json(outcome: &sandbox::Outcome) -> Json {
    outcome.exit.map_or(Json::Null, |code| Json::Num(code.to_string()))
}

fn run_response(mut outcome: sandbox::Outcome) -> Response {
    loop {
        let body = Json::obj(vec![
            ("stdout", Json::str(&outcome.stdout)),
            ("stderr", Json::str(&outcome.stderr)),
            ("exit", exit_json(&outcome)),
            ("killed", outcome.killed.map_or(Json::Null, Json::str)),
            ("truncated", Json::Bool(outcome.truncated)),
            ("bytes", num(outcome.bytes)),
            ("est_tokens", num(est_tokens(outcome.bytes))),
            ("ms", num(outcome.ms)),
        ])
        .to_line();
        if body.len() <= MAX_RESPONSE || outcome.stdout.is_empty() {
            return Response::json(200, body);
        }
        let mut keep = outcome.stdout.len() / 2;
        while !outcome.stdout.is_char_boundary(keep) {
            keep -= 1;
        }
        outcome.stdout.truncate(keep);
        outcome.truncated = true;
    }
}

fn mcp_response(outcome: sandbox::Outcome) -> Response {
    let responses: Vec<Json> =
        outcome.stdout.lines().filter(|l| !l.trim().is_empty()).map(|l| json::parse(l).unwrap_or_else(|_| Json::str(l))).collect();
    let body = Json::obj(vec![
        ("responses", Json::Arr(responses)),
        ("stderr", Json::str(&outcome.stderr)),
        ("exit", exit_json(&outcome)),
        ("killed", outcome.killed.map_or(Json::Null, Json::str)),
        ("truncated", Json::Bool(outcome.truncated)),
        ("ms", num(outcome.ms)),
    ])
    .to_line();
    if body.len() > MAX_RESPONSE {
        return error(500, "the MCP answer is larger than the response limit");
    }
    Response::json(200, body)
}

fn form_response(title: &str, outcome: sandbox::Outcome) -> Response {
    let exit = outcome.exit.map_or_else(|| format!("killed ({})", outcome.killed.unwrap_or("signal")), |c| format!("exit {c}"));
    let meta = format!(
        "{exit} · {} bytes · ~{} est. tokens · {} ms{}",
        outcome.bytes,
        est_tokens(outcome.bytes),
        outcome.ms,
        if outcome.truncated { " · output truncated" } else { "" }
    );
    let html = form::page(title, &meta, &[("stdout", &outcome.stdout), ("stderr", &outcome.stderr)]);
    Response::new(200, "text/html; charset=utf-8", html).with("Content-Security-Policy", FORM_CSP)
}

fn serve(app: &App, mut stream: TcpStream) {
    let peer = stream.peer_addr().map(|a| a.ip().to_string()).unwrap_or_default();
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    let response = match http::read_request(&mut stream, Duration::from_secs(10)) {
        Ok(req) => {
            let mut response = app.handle(&req, &peer);
            if req.method == "HEAD" {
                let length = response.body.len();
                response.body.clear();
                response = response.with("X-Body-Length", length.to_string());
            }
            response
        }
        Err(ReadError::Bad(why)) => error(400, why),
        Err(ReadError::LengthRequired) => error(411, "send a Content-Length body"),
        Err(ReadError::TooLarge) => error(413, "the request is larger than 512 KiB"),
        Err(ReadError::Closed) => return,
    };
    let _ = response.write_to(&mut stream);
    let _ = stream.shutdown(Shutdown::Write);
}

fn health_probe(addr: &str) -> i32 {
    let addr = addr.replace("0.0.0.0", "127.0.0.1");
    let Ok(mut stream) = TcpStream::connect(&addr) else {
        return 1;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    if stream.write_all(b"GET /api/v1/health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").is_err() {
        return 1;
    }
    let mut head = [0u8; 12];
    match std::io::Read::read_exact(&mut stream, &mut head) {
        Ok(()) if &head == b"HTTP/1.1 200" => 0,
        _ => 1,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--health") => std::process::exit(health_probe(args.get(1).map_or("127.0.0.1:8080", String::as_str))),
        Some("--version") => {
            println!("dctx-play {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        Some("--help" | "-h") => {
            print!("{USAGE}");
            return;
        }
        _ => {}
    }
    let cfg = match parse_args(&args) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("dctx-play: {e}");
            std::process::exit(2);
        }
    };
    let app = match App::new(cfg) {
        Ok(app) => Arc::new(app),
        Err(e) => {
            eprintln!("dctx-play: {e}");
            std::process::exit(1);
        }
    };
    let listener = match TcpListener::bind(&app.cfg.listen) {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("dctx-play: cannot listen on {}: {e}", app.cfg.listen);
            std::process::exit(1);
        }
    };
    // Accepted connections wait in a bounded queue for one of the worker threads; when the
    // queue is full the accepting thread answers 503 itself.
    let (sender, receiver) = sync_channel::<TcpStream>(app.cfg.queue);
    let receiver = Arc::new(Mutex::new(receiver));
    for _ in 0..app.cfg.threads {
        let (app, receiver) = (Arc::clone(&app), Arc::clone(&receiver));
        std::thread::spawn(move || loop {
            let next = receiver.lock().unwrap_or_else(|e| e.into_inner()).recv();
            match next {
                Ok(stream) => serve(&app, stream),
                Err(_) => return,
            }
        });
    }
    eprintln!(
        "dctx-play: listening on {} with {} samples, dctx at {}",
        // The bound address, so `--listen HOST:0` reports the port the kernel chose.
        listener
            .local_addr()
            .map_or_else(|_| app.cfg.listen.clone(), |addr| addr.to_string()),
        app.samples.len(),
        app.cfg.dctx.display()
    );
    for stream in listener.incoming().flatten() {
        match sender.try_send(stream) {
            Ok(()) => {}
            Err(TrySendError::Full(mut stream)) => {
                app.metrics.reject("busy");
                // Read what the client already sent so closing does not reset the connection.
                let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
                let mut sink = [0u8; 8192];
                let _ = std::io::Read::read(&mut stream, &mut sink);
                let _ = error(503, "the playground is busy; try again in a few seconds").with("Retry-After", "2").write_to(&mut stream);
                let _ = stream.shutdown(Shutdown::Write);
                while matches!(std::io::Read::read(&mut stream, &mut sink), Ok(n) if n > 0) {}
            }
            Err(TrySendError::Disconnected(_)) => break,
        }
    }
}

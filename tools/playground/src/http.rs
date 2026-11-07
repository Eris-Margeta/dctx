// SPDX-License-Identifier: Apache-2.0
//! A deliberately small HTTP/1.1 subset: one request per connection, `Content-Length` bodies only.

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// Largest request head (request line and headers).
pub const MAX_HEAD: usize = 16 * 1024;
/// Largest request body.
pub const MAX_BODY: usize = 512 * 1024;

/// A parsed request.
#[derive(Debug)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    /// The first header named `name` (lower case).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    }
}

/// Why a request could not be read; each maps to one status code.
#[derive(Debug, PartialEq, Eq)]
pub enum ReadError {
    /// Malformed request line or header.
    Bad(&'static str),
    /// A POST without `Content-Length`.
    LengthRequired,
    /// Head or body over the limit.
    TooLarge,
    /// The peer went away or timed out; nothing is answered.
    Closed,
}

/// Parses a request head (everything before the blank line).
pub fn parse_head(head: &str) -> Result<(String, String, Vec<(String, String)>), ReadError> {
    let mut lines = head.split("\r\n");
    let request_line = lines.next().ok_or(ReadError::Bad("empty request"))?;
    let mut parts = request_line.split(' ');
    let (method, target, version) = match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(m), Some(t), Some(v), None) => (m, t, v),
        _ => return Err(ReadError::Bad("malformed request line")),
    };
    if !method.bytes().all(|b| b.is_ascii_uppercase()) || method.is_empty() {
        return Err(ReadError::Bad("malformed method"));
    }
    if version != "HTTP/1.1" && version != "HTTP/1.0" {
        return Err(ReadError::Bad("unsupported HTTP version"));
    }
    if !target.starts_with('/') || target.bytes().any(|b| b <= b' ' || b == 0x7f) {
        return Err(ReadError::Bad("malformed request target"));
    }
    let path = target.split(['?', '#']).next().unwrap_or("/").to_string();
    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let (name, value) = line.split_once(':').ok_or(ReadError::Bad("malformed header"))?;
        if name.is_empty() || name.bytes().any(|b| !(b.is_ascii_alphanumeric() || b == b'-' || b == b'_')) {
            return Err(ReadError::Bad("malformed header name"));
        }
        headers.push((name.to_ascii_lowercase(), value.trim().to_string()));
        if headers.len() > 64 {
            return Err(ReadError::TooLarge);
        }
    }
    Ok((method.to_string(), path, headers))
}

/// Reads one request from `stream` with the given socket timeout.
pub fn read_request(stream: &mut TcpStream, timeout: Duration) -> Result<Request, ReadError> {
    stream.set_read_timeout(Some(timeout)).map_err(|_| ReadError::Closed)?;
    let mut buffer = Vec::with_capacity(2048);
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(at) = find(&buffer, b"\r\n\r\n") {
            break at;
        }
        if buffer.len() > MAX_HEAD {
            return Err(ReadError::TooLarge);
        }
        let n = stream.read(&mut chunk).map_err(|_| ReadError::Closed)?;
        if n == 0 {
            return Err(ReadError::Closed);
        }
        buffer.extend_from_slice(&chunk[..n]);
    };
    if head_end > MAX_HEAD {
        return Err(ReadError::TooLarge);
    }
    let head = std::str::from_utf8(&buffer[..head_end]).map_err(|_| ReadError::Bad("request head is not UTF-8"))?;
    let (method, path, headers) = parse_head(head)?;
    let mut body = buffer[head_end + 4..].to_vec();
    let length = headers.iter().find(|(k, _)| k == "content-length").map(|(_, v)| v.as_str());
    if headers.iter().any(|(k, _)| k == "transfer-encoding") {
        return Err(ReadError::LengthRequired);
    }
    let length = match length {
        Some(text) => text.parse::<usize>().map_err(|_| ReadError::Bad("malformed Content-Length"))?,
        None if method == "POST" => return Err(ReadError::LengthRequired),
        None => 0,
    };
    if length > MAX_BODY {
        return Err(ReadError::TooLarge);
    }
    while body.len() < length {
        let n = stream.read(&mut chunk).map_err(|_| ReadError::Closed)?;
        if n == 0 {
            return Err(ReadError::Closed);
        }
        body.extend_from_slice(&chunk[..n]);
        if body.len() > MAX_BODY {
            return Err(ReadError::TooLarge);
        }
    }
    body.truncate(length);
    Ok(Request { method, path, headers, body })
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

/// A response ready to write.
#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn new(status: u16, content_type: &'static str, body: impl Into<Vec<u8>>) -> Self {
        Response { status, content_type, headers: Vec::new(), body: body.into() }
    }

    pub fn json(status: u16, body: String) -> Self {
        Self::new(status, "application/json; charset=utf-8", body)
    }

    pub fn with(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.headers.push((name, value.into()));
        self
    }

    /// Serialises status line, headers and body.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nX-Content-Type-Options: nosniff\r\n",
            self.status,
            reason(self.status),
            self.content_type,
            self.body.len()
        );
        if !self.headers.iter().any(|(name, _)| *name == "Cache-Control") {
            out.push_str("Cache-Control: no-store\r\n");
        }
        for (name, value) in &self.headers {
            out.push_str(name);
            out.push_str(": ");
            out.push_str(value);
            out.push_str("\r\n");
        }
        out.push_str("\r\n");
        let mut bytes = out.into_bytes();
        bytes.extend_from_slice(&self.body);
        bytes
    }

    pub fn write_to(&self, stream: &mut TcpStream) -> io::Result<()> {
        stream.write_all(&self.to_bytes())?;
        stream.flush()
    }
}

/// The reason phrase for the status codes this server sends.
pub fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        411 => "Length Required",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_request_head() {
        let (method, path, headers) =
            parse_head("POST /api/v1/run?x=1 HTTP/1.1\r\nHost: a\r\nContent-Length: 5").unwrap();
        assert_eq!(method, "POST");
        assert_eq!(path, "/api/v1/run");
        assert_eq!(headers[1], ("content-length".to_string(), "5".to_string()));
    }

    #[test]
    fn refuses_malformed_heads() {
        for head in [
            "",
            "GET /",
            "GET / HTTP/2.0",
            "get / HTTP/1.1",
            "GET relative HTTP/1.1",
            "GET / HTTP/1.1 extra",
            "GET / HTTP/1.1\r\nno colon",
            "GET / HTTP/1.1\r\nBad Name: x",
        ] {
            assert!(parse_head(head).is_err(), "{head:?} was accepted");
        }
    }

    #[test]
    fn response_has_length_and_no_store() {
        let text = String::from_utf8(Response::json(200, "{}".into()).to_bytes()).unwrap();
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(text.contains("Content-Length: 2\r\n"));
        assert!(text.contains("Cache-Control: no-store\r\n"));
        assert!(text.ends_with("\r\n\r\n{}"));
    }
}

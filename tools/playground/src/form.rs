// SPDX-License-Identifier: Apache-2.0
//! The no-JavaScript path: `application/x-www-form-urlencoded` input and a plain HTML result page.

use crate::grammar::File;

/// Decodes `a=b&c=d` (with `+` and `%XX`) into ordered pairs.
pub fn parse_urlencoded(body: &str) -> Result<Vec<(String, String)>, String> {
    let mut pairs = Vec::new();
    for part in body.split('&').filter(|p| !p.is_empty()) {
        let (key, value) = part.split_once('=').unwrap_or((part, ""));
        pairs.push((decode(key)?, decode(value)?));
        if pairs.len() > 16 {
            return Err("too many form fields".into());
        }
    }
    Ok(pairs)
}

fn decode(text: &str) -> Result<String, String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' => {
                let hex = text.get(i + 1..i + 3).ok_or("truncated % escape")?;
                out.push(u8::from_str_radix(hex, 16).map_err(|_| "bad % escape")?);
                i += 2;
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8(out).map_err(|_| "form data is not UTF-8".into())
}

/// Parses the files textarea: each file starts with a line `--- path ---`.
pub fn parse_files(text: &str) -> Result<Vec<File>, String> {
    let text = text.replace("\r\n", "\n");
    let mut files: Vec<File> = Vec::new();
    for line in text.split_inclusive('\n') {
        let bare = line.trim_end_matches('\n');
        if let Some(path) = bare.strip_prefix("--- ").and_then(|rest| rest.strip_suffix(" ---")) {
            files.push(File { path: path.trim().to_string(), content: String::new() });
        } else if let Some(file) = files.last_mut() {
            file.content.push_str(line);
        } else if !bare.trim().is_empty() {
            return Err("start each file with a line `--- path/to/file ---`".into());
        }
    }
    Ok(files)
}

/// Escapes text for HTML element content and attribute values.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// The result page. `meta` is one line of facts, `blocks` are (label, preformatted text).
pub fn page(title: &str, meta: &str, blocks: &[(&str, &str)]) -> String {
    let mut body = String::new();
    for (label, text) in blocks {
        if text.is_empty() {
            continue;
        }
        body.push_str(&format!("<h2>{}</h2>\n<pre>{}</pre>\n", escape(label), escape(text)));
    }
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
<meta name=\"robots\" content=\"noindex\">\n<meta name=\"color-scheme\" content=\"light dark\">\n<title>{t} · dctx playground</title>\n\
<link rel=\"stylesheet\" href=\"/api/v1/form.css\">\n</head>\n<body>\n<main>\n<p><a href=\"/playground/\">&larr; Back to the playground</a></p>\n\
<h1>{t}</h1>\n<p class=\"meta\">{m}</p>\n{body}</main>\n</body>\n</html>\n",
        t = escape(title),
        m = escape(meta)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_form_fields() {
        let pairs = parse_urlencoded("cmd=map+find+%27a+b%27&sample=kiosk&empty=").unwrap();
        assert_eq!(pairs[0], ("cmd".into(), "map find 'a b'".into()));
        assert_eq!(pairs[2], ("empty".into(), String::new()));
        assert!(parse_urlencoded("a=%zz").is_err());
        assert!(parse_urlencoded("a=%e").is_err());
        assert!(parse_urlencoded("a=%ff").is_err(), "invalid UTF-8");
    }

    #[test]
    fn splits_the_files_textarea() {
        let files = parse_files("--- src/a.rs ---\nfn a() {}\n--- b.py ---\r\ndef b():\r\n    pass\r\n").unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0], File { path: "src/a.rs".into(), content: "fn a() {}\n".into() });
        assert_eq!(files[1].content, "def b():\n    pass\n");
        assert!(parse_files("loose text\n--- a ---\n").is_err());
        assert!(parse_files("\n\n").unwrap().is_empty());
    }

    #[test]
    fn page_escapes_everything() {
        let html = page("<x>", "a & b", &[("stdout", "<script>"), ("stderr", "")]);
        assert!(html.contains("&lt;script&gt;"));
        assert!(!html.contains("<script>"));
        assert!(!html.contains("<h2>stderr</h2>"));
    }
}

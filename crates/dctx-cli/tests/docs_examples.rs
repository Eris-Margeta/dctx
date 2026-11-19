// SPDX-License-Identifier: Apache-2.0

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use html5ever::tokenizer::{BufferQueue, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer};
use percent_encoding::percent_decode_str;

#[cfg(unix)]
mod common;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn documentation(root: &Path) -> Vec<PathBuf> {
    let mut paths = vec![root.join("README.md")];
    paths.extend(
        fs::read_dir(root.join("docs"))
            .expect("docs directory")
            .map(|entry| entry.expect("docs entry").path())
            .filter(|path| is_markdown(path) && path.is_file()),
    );
    paths.sort();
    paths
}

fn is_markdown(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "md")
}

fn read_markdown(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

#[cfg(unix)]
#[test]
fn docs_examples_run_offline() {
    use markdown::mdast::Node;
    use std::{
        io::Write,
        process::{Command, Stdio},
    };

    let root = repo_root();
    let mut blocks = 0;
    let mut failures = Vec::new();
    for path in documentation(&root) {
        let source = read_markdown(&path);
        let document = markdown::to_mdast(&source, &markdown::ParseOptions::gfm()).unwrap();
        let mut nodes = vec![&document];
        while let Some(node) = nodes.pop() {
            if let Some(children) = node.children() {
                nodes.extend(children.iter().rev());
            }
            let Node::Code(code) = node else { continue };
            if !matches!(code.lang.as_deref(), Some("sh" | "shell" | "bash" | "zsh"))
                || code.meta.as_deref() != Some("runnable")
            {
                continue;
            }
            let shell = match code.lang.as_deref() {
                Some("bash") => "/bin/bash",
                Some("zsh") => "/bin/zsh",
                _ => "/bin/sh",
            };
            let location = format!(
                "{}:{}",
                path.strip_prefix(&root).unwrap().display(),
                code.position.as_ref().unwrap().start.line
            );
            assert!(
                !code.value.trim().is_empty(),
                "{location}: empty runnable block"
            );
            blocks += 1;

            let temp = tempfile::tempdir().expect("example directory");
            let project = temp.path().join("project");
            fs::create_dir_all(project.join("src")).unwrap();
            fs::write(
                project.join("src/lib.rs"),
                "pub struct Config;\npub struct Session;\npub fn ping() {}\npub fn alpha_body() {}\n",
            )
            .unwrap();
            let package = project.join("packages/service-a");
            fs::create_dir_all(package.join("src")).unwrap();
            fs::write(package.join("src/lib.rs"), "pub struct Config;\n").unwrap();
            let fixture = "crates/dctx-lex-go/tests/fixtures/basic.go";
            fs::create_dir_all(project.join(fixture).parent().unwrap()).unwrap();
            fs::copy(root.join(fixture), project.join(fixture)).unwrap();

            let mut command = Command::new(shell);
            command
                .args([
                    "-eu",
                    "-c",
                    "dctx() { \"$DCTX_DOCS_BIN\" \"$@\"; }\n. \"$1\"",
                ])
                .current_dir(&project)
                .env_clear()
                .env("PATH", temp.path().join("bin"))
                .env("DCTX_DOCS_BIN", env!("CARGO_BIN_EXE_dctx-cli"))
                .stdin(Stdio::null());
            common::isolated_dirs(&mut command, temp.path());

            // Keep the script outside the scan root and let the shell read it by absolute path.
            let mut script = tempfile::NamedTempFile::new_in(temp.path()).unwrap();
            writeln!(script, "{}", code.value).unwrap();
            let output = command
                .args(["docs-example"])
                .arg(script.path())
                .output()
                .unwrap_or_else(|error| panic!("{location}: {error}"));
            if !output.status.success() {
                failures.push(format!(
                    "{location}: {}\n{}\nstdout:\n{}\nstderr:\n{}",
                    output.status,
                    code.value,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
        }
    }
    assert!(blocks > 0, "no runnable shell fences found");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    eprintln!("Ran {blocks} runnable shell blocks");
}

#[derive(Default)]
struct Page {
    links: Vec<String>,
    anchors: HashSet<String>,
    headings: HashSet<String>,
    heading: Option<String>,
}

struct PageSink(RefCell<Page>);

impl TokenSink for PageSink {
    type Handle = ();

    fn process_token(&self, token: Token, _: u64) -> TokenSinkResult<()> {
        let mut page = self.0.borrow_mut();
        match token {
            Token::TagToken(tag) => {
                let name = tag.name.as_ref();
                let heading = matches!(name, "h1" | "h2" | "h3" | "h4" | "h5" | "h6");
                if tag.kind == TagKind::StartTag {
                    if heading {
                        page.heading = Some(String::new());
                    }
                    for attr in tag.attrs {
                        match attr.name.local.as_ref() {
                            "href" | "src" => page.links.push(attr.value.to_string()),
                            "id" => {
                                page.anchors.insert(attr.value.to_string());
                            }
                            "name" if name == "a" => {
                                page.anchors.insert(attr.value.to_string());
                            }
                            _ => {}
                        }
                    }
                } else if heading {
                    let Some(title) = page.heading.take() else {
                        return TokenSinkResult::Continue;
                    };
                    let slug: String = title
                        .trim()
                        .to_lowercase()
                        .chars()
                        .filter_map(|ch| match ch {
                            ' ' => Some('-'),
                            '-' | '_' => Some(ch),
                            _ if ch.is_alphanumeric() => Some(ch),
                            _ => None,
                        })
                        .collect();
                    let mut anchor = slug.clone();
                    let mut suffix = 1;
                    while !page.headings.insert(anchor.clone()) {
                        anchor = format!("{slug}-{suffix}");
                        suffix += 1;
                    }
                    page.anchors.insert(anchor);
                }
            }
            Token::CharacterTokens(text) => {
                if let Some(title) = &mut page.heading {
                    title.push_str(&text);
                }
            }
            _ => {}
        }
        TokenSinkResult::Continue
    }
}

fn parse_page(source: &str) -> Page {
    let mut options = markdown::Options::gfm();
    options.compile.allow_dangerous_html = true;
    let html = markdown::to_html_with_options(source, &options).unwrap();
    let input = BufferQueue::default();
    input.push_back(html.into());
    let tokenizer = Tokenizer::new(PageSink(RefCell::default()), Default::default());
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    tokenizer.sink.0.into_inner()
}

fn is_external(url: &str) -> bool {
    url.starts_with("//")
        || url
            .split(['/', '?', '#'])
            .next()
            .is_some_and(|first| first.contains(':'))
}

fn check_link(
    root: &Path,
    source: &Path,
    url: &str,
    anchors: &mut HashMap<PathBuf, HashSet<String>>,
) -> Result<(), String> {
    let (path, fragment) = url.split_once('#').unwrap_or((url, ""));
    let path = path.split('?').next().unwrap();
    let path = percent_decode_str(path)
        .decode_utf8()
        .map_err(|error| error.to_string())?;
    let target = if path.is_empty() {
        source.to_path_buf()
    } else if let Some(path) = path.strip_prefix('/') {
        root.join(path)
    } else {
        source.parent().unwrap().join(path.as_ref())
    };
    let target = target.canonicalize().map_err(|_| "missing target")?;
    if !target.starts_with(root) {
        return Err("target is outside the workspace".into());
    }
    if !fragment.is_empty() && is_markdown(&target) {
        let fragment = percent_decode_str(fragment)
            .decode_utf8()
            .map_err(|error| error.to_string())?;
        let known = anchors
            .entry(target.clone())
            .or_insert_with(|| parse_page(&read_markdown(&target)).anchors);
        if !known.contains(fragment.as_ref()) {
            return Err(format!("missing anchor #{fragment}"));
        }
    }
    Ok(())
}

#[test]
fn docs_examples_relative_links_resolve() {
    let root = repo_root();
    let mut anchors = HashMap::new();
    let mut failures = Vec::new();
    let mut links = 0;
    for path in documentation(&root) {
        let page = parse_page(&read_markdown(&path));
        anchors.insert(path.clone(), page.anchors);
        for url in page.links.iter().filter(|url| !is_external(url)) {
            links += 1;
            if let Err(error) = check_link(&root, &path, url, &mut anchors) {
                failures.push(format!(
                    "{}: {url}: {error}",
                    path.strip_prefix(&root).unwrap().display()
                ));
            }
        }
    }
    assert!(links > 0, "no relative documentation links found");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    eprintln!("Checked {links} relative links");
}

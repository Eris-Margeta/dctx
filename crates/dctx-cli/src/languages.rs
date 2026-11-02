//! `dctx languages`: every supported language with its tag, file names, tier and symbol confidence.
//! Driven only by the facade registry [`dctx::LANGUAGES`]: the text table, `--json` and `--markdown`, which is the README
//! block between `<!-- languages:begin -->` and `<!-- languages:end -->`. A unit test diffs that block against [`markdown`].
//! Reads no source, opens no cache and starts nothing.

use std::fmt::Write as _;

use dctx::{Language, Tier, LANGUAGES};

/// Help page of `dctx languages`.
pub const HELP: &str = "\
Usage: dctx languages [--json | --markdown]

Lists every language the map index recognises: name, tag, file names, tier and symbol confidence.
Tiers: full = symbols and call edges (lexical, not type-resolved); outline = symbols only, no call edges promised;
docs = headed sections. Confidence `guess` marks heuristic outlines, whose records carry confidence `guess`.

Options:
  --json        One JSON document with the tiers and the languages
  --markdown    The README language table
  -h, --help    Show this help

Example: dctx languages --json
";

/// Opening marker of the generated README block.
pub const README_BEGIN: &str = "<!-- languages:begin -->\n";
/// Closing marker of the generated README block.
pub const README_END: &str = "<!-- languages:end -->";

/// Output format of one run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    /// Aligned text table.
    Text,
    /// One JSON document.
    Json,
    /// The README Markdown table.
    Markdown,
}

/// Runs `dctx languages` with the arguments after the subcommand and prints the page.
///
/// # Errors
///
/// An unknown argument or more than one format flag; nothing is printed then.
pub fn run(arguments: &[String]) -> Result<(), String> {
    print!("{}", page(arguments)?);
    Ok(())
}

/// The page `dctx languages` prints for `arguments`.
///
/// # Errors
///
/// An unknown argument or more than one format flag.
pub fn page(arguments: &[String]) -> Result<String, String> {
    let mut format = None;
    for argument in arguments {
        let next = match argument.as_str() {
            "-h" | "--help" => return Ok(HELP.to_string()),
            "--json" => Format::Json,
            "--markdown" => Format::Markdown,
            other => return Err(format!("unknown argument `{other}`; see `dctx languages --help`")),
        };
        if format.replace(next).is_some() {
            return Err("give at most one of --json and --markdown".to_string());
        }
    }
    Ok(match format.unwrap_or(Format::Text) {
        Format::Text => text(),
        Format::Json => json(),
        Format::Markdown => markdown(),
    })
}

/// File-name cells of one language: `.ext` for each extension, then each whole file name or `*` suffix.
fn file_names(language: &Language) -> Vec<String> {
    language.extensions.iter().map(|extension| format!(".{extension}")).chain(language.files.iter().map(|file| (*file).to_string())).collect()
}

/// Aligned text table, then one line per tier.
#[must_use]
pub fn text() -> String {
    let tag_width = LANGUAGES.iter().map(|language| language.tag.len()).max().unwrap_or(0).max("tag".len());
    let name_width = LANGUAGES.iter().map(|language| language.name.len()).max().unwrap_or(0).max("language".len());
    let mut out = String::from("# dctx languages\n");
    let _ = writeln!(out, "{:<7}  {:<10}  {:<tag_width$}  {:<name_width$}  files", "tier", "confidence", "tag", "language");
    for language in LANGUAGES {
        let _ = writeln!(
            out,
            "{:<7}  {:<10}  {:<tag_width$}  {:<name_width$}  {}",
            language.tier.as_str(),
            language.confidence.as_str(),
            language.tag,
            language.name,
            file_names(language).join(" ")
        );
    }
    out.push('\n');
    for tier in Tier::ALL {
        let _ = writeln!(out, "{}: {}", tier.as_str(), tier.meaning());
    }
    out.push_str("guess: heuristic outline; every record of these files carries confidence `guess`.\n");
    out
}

/// One JSON document: `{"tiers":[{"tier","meaning"}...],"languages":[{"name","tag","extensions","files","tier","confidence"}...]}`.
#[must_use]
pub fn json() -> String {
    let mut out = String::from("{\"tiers\":[");
    for (index, tier) in Tier::ALL.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"tier\":");
        quote(&mut out, tier.as_str());
        out.push_str(",\"meaning\":");
        quote(&mut out, tier.meaning());
        out.push('}');
    }
    out.push_str("],\"languages\":[");
    for (index, language) in LANGUAGES.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"name\":");
        quote(&mut out, language.name);
        out.push_str(",\"tag\":");
        quote(&mut out, language.tag);
        out.push_str(",\"extensions\":");
        list(&mut out, language.extensions);
        out.push_str(",\"files\":");
        list(&mut out, language.files);
        out.push_str(",\"tier\":");
        quote(&mut out, language.tier.as_str());
        out.push_str(",\"confidence\":");
        quote(&mut out, language.confidence.as_str());
        out.push('}');
    }
    out.push_str("]}\n");
    out
}

/// The README language table: the exact text between [`README_BEGIN`] and [`README_END`].
#[must_use]
pub fn markdown() -> String {
    let mut out = String::from("| Language | Tag | Files | Tier | Confidence |\n|---|---|---|---|---|\n");
    for language in LANGUAGES {
        let files: Vec<String> = file_names(language).iter().map(|name| format!("`{name}`")).collect();
        let _ = writeln!(
            out,
            "| {} | `{}` | {} | {} | {} |",
            language.name,
            language.tag,
            files.join(", "),
            language.tier.as_str(),
            language.confidence.as_str()
        );
    }
    out
}

/// The generated block of a README text, or `None` without exactly one pair of markers in order.
#[must_use]
pub fn readme_block(readme: &str) -> Option<&str> {
    if readme.matches(README_BEGIN).count() != 1 || readme.matches(README_END).count() != 1 {
        return None;
    }
    let (_, rest) = readme.split_once(README_BEGIN)?;
    let (block, _) = rest.split_once(README_END)?;
    Some(block)
}

/// Appends `items` as a JSON array of strings.
fn list(out: &mut String, items: &[&str]) {
    out.push('[');
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        quote(out, item);
    }
    out.push(']');
}

/// Appends `value` as a JSON string.
fn quote(out: &mut String, value: &str) {
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            control if u32::from(control) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(control));
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_string()).collect()
    }

    #[test]
    fn readme_block_is_the_rendered_registry() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../README.md");
        let readme = std::fs::read_to_string(path).expect("README.md");
        let block = readme_block(&readme).expect("README needs one languages:begin and one languages:end marker");
        assert_eq!(block, markdown(), "README language table is stale: regenerate it with `dctx languages --markdown`");
    }

    #[test]
    fn readme_block_needs_one_ordered_marker_pair() {
        assert_eq!(readme_block("a\n<!-- languages:begin -->\n| x |\n<!-- languages:end -->\nb"), Some("| x |\n"));
        assert_eq!(readme_block("no markers"), None);
        assert_eq!(readme_block("<!-- languages:end -->\n<!-- languages:begin -->\n"), None);
        assert_eq!(readme_block("<!-- languages:begin -->\n<!-- languages:begin -->\n<!-- languages:end -->"), None);
    }

    #[test]
    fn help_has_one_example_line() {
        let help = page(&args(&["--help"])).expect("help");
        assert_eq!(help.matches("Example:").count(), 1);
        assert_eq!(page(&args(&["-h"])).expect("help"), help);
    }

    #[test]
    fn formats_follow_the_flags_and_bad_arguments_are_refused() {
        assert_eq!(page(&[]).expect("text"), text());
        assert_eq!(page(&args(&["--json"])).expect("json"), json());
        assert_eq!(page(&args(&["--markdown"])).expect("markdown"), markdown());
        assert!(page(&args(&["--bogus"])).expect_err("unknown").contains("--help"));
        assert!(page(&args(&["--json", "--markdown"])).is_err());
        assert!(page(&args(&["--json", "--json"])).is_err());
    }

    #[test]
    fn every_output_lists_every_registry_entry() {
        let (text, json, markdown) = (text(), json(), markdown());
        assert_eq!(json.matches("\"tag\":").count(), LANGUAGES.len());
        assert_eq!(markdown.lines().count(), LANGUAGES.len() + 2);
        assert_eq!(text.lines().filter(|line| LANGUAGES.iter().any(|language| line.split_whitespace().nth(2) == Some(language.tag))).count(), LANGUAGES.len());
        assert!(json.contains(
            "{\"name\":\"Shell\",\"tag\":\"shell\",\"extensions\":[\"sh\",\"bash\",\"zsh\",\"bats\"],\"files\":[],\"tier\":\"outline\",\"confidence\":\"guess\"}"
        ));
        assert!(json.contains("\"files\":[\"Dockerfile\",\"Containerfile\"]"));
        assert!(markdown.contains("| Rust | `rust` | `.rs` | full | structural |\n"));
        assert!(markdown.contains("| Markdown | `markdown` | `.md`, `.markdown` | docs | structural |\n"));
        assert!(text.contains("Perl") && json.contains("Perl") && markdown.contains("Perl"), "Perl is listed");
        assert!(markdown.contains("| Perl | `perl` | `.pl`, `.pm`, `.t`, `.psgi` | full | guess |\n"));
        for gone in ["Fortran", "Verilog"] {
            assert!(!text.contains(gone) && !json.contains(gone) && !markdown.contains(gone), "{gone}");
        }
    }

    #[test]
    fn json_strings_are_escaped() {
        let mut out = String::new();
        quote(&mut out, "a\"b\\c\nd");
        assert_eq!(out, "\"a\\\"b\\\\c\\u000ad\"");
    }
}

//! Tests for web files: script blocks keep original lines and spans, headings become doc sections, unclosed tags are safe.

use super::lexed;
use crate::index::extract::{extract_structural_refs, language_of};

const VUE: &str = "<template>\n  <div>{{ msg }}</div>\n</template>\n<script setup lang=\"ts\">\nfunction greet(name: string): string {\n  return \"hi \" + name\n}\nfunction run() {\n  greet(\"x\")\n}\n</script>\n";

#[test]
fn extensions_share_the_web_tag() {
    for extension in ["vue", "svelte", "astro", "html", "htm", "HTML"] {
        assert_eq!(language_of(&format!("src/a.{extension}")), "web", "{extension}");
    }
}

#[test]
fn script_symbols_keep_original_lines_spans_and_references() {
    let (symbols, refs) = lexed("C.vue", VUE);
    let greet = symbols.iter().find(|symbol| symbol.name == "greet").expect("greet");
    assert_eq!(greet.lines, [5, 7]);
    let start = usize::try_from(greet.span[0]).expect("start");
    assert!(VUE[start..].starts_with("function greet"));
    let call = refs.iter().find(|reference| reference.name == "greet").expect("call");
    assert_eq!(call.src, "s:C.vue#run");
}

#[test]
fn svelte_astro_and_html_find_their_scripts_and_headings() {
    let svelte = "<script>\n  export function toggle() {\n    open = !open\n  }\n</script>\n<h1>Title</h1>\n";
    let names: Vec<_> = extract_structural_refs("S.svelte", svelte.as_bytes()).0.into_iter().map(|symbol| (symbol.name, symbol.kind)).collect();
    assert_eq!(names, [("toggle".to_string(), "fn".to_string()), ("Title".to_string(), "heading".to_string())]);
    let astro = "---\nfunction load() { return 1 }\n---\n<h1>Hi</h1>\n";
    let found = extract_structural_refs("A.astro", astro.as_bytes()).0;
    assert_eq!(found.iter().map(|symbol| symbol.name.as_str()).collect::<Vec<_>>(), ["load", "Hi"]);
    let html = "<html><body>\n<h1>Install <b>guide</b></h1>\n<p>text</p>\n<H2 class=\"x\">Linux &amp; setup</H2>\n<script>\nfunction boot() { return 1 }\n</script>\n<!-- <h2>Hidden</h2> -->\n</body></html>\n";
    let found = lexed("page.html", html).0;
    let titles: Vec<_> = found.iter().map(|symbol| (symbol.name.as_str(), symbol.lines)).collect();
    assert_eq!(titles, [("Install guide", [2, 9]), ("Linux & setup", [4, 9]), ("boot", [6, 6])]);
    assert_eq!(found[0].kind, "heading");
}

#[test]
fn data_scripts_are_not_lexed_and_unclosed_tags_never_panic() {
    let json = "<script type=\"application/json\">\nfunction nope() {}\n</script>\n";
    assert!(lexed("a.html", json).0.is_empty());
    for broken in ["<script>function a() {", "<script", "<h1>never closed", "<h2", "<!-- open", "<style>p {", "---\nfunction a() {}\n", "<script src=\"x.js\" />\n<h3>ok</h3>"] {
        let (symbols, _) = lexed("a.html", broken);
        assert!(symbols.len() <= 1, "{broken}: {symbols:?}");
    }
    assert_eq!(lexed("a.html", "<script src=\"x.js\" />\n<h3>ok</h3>").0[0].name, "ok");
}

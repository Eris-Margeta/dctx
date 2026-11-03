// SPDX-License-Identifier: Apache-2.0
//! Author-written tests for DCX-54b. Do not edit.
use super::calls_in;

fn names(found: &[(String, Vec<u32>)]) -> Vec<&str> {
    found.iter().map(|(name, _)| name.as_str()).collect()
}

#[test]
fn python_receiver_methods_builtins_and_comments() {
    let text = "    def get_filters(self, request):\n        # fake_comment(1)\n        if self.lookup_allowed(request):\n            helper(1)\n        for f in request.items():\n            if callable(f):\n                f.has_output()\n        return helper(2)\n";
    let found = calls_in("python", text, 5, "get_filters");
    assert_eq!(names(&found), ["lookup_allowed", "helper", "items", "callable", "has_output"]);
    assert_eq!(found[0].1, vec![7]);
    assert_eq!(found[1].1, vec![8, 12]);
    assert_eq!(found[4].1, vec![11]);
}

#[test]
fn rust_paths_and_method_chains() {
    let text = "pub fn is_fast(&self) -> bool {\n    let lt = self.config.line_terminator();\n    lt.is_some() && self.non_matching_bytes().len() == 0 && match lt { _ => true } && String::new().is_empty()\n}\n";
    let found = calls_in("rust", text, 3, "is_fast");
    assert_eq!(names(&found), ["line_terminator", "is_some", "non_matching_bytes", "len", "new", "is_empty"]);
    assert_eq!(found[0].1, vec![4]);
}

#[test]
fn strings_and_comments_are_masked_and_lines_are_capped() {
    let text = "function run() {\n  log(\"call(me)\");\n  log(1);\n  log(2);\n  log(3);\n  // skip(1)\n  /* skip2(2) */\n  return x.Validate();\n}\n";
    let found = calls_in("javascript", text, 1, "run");
    assert_eq!(names(&found), ["log", "Validate"]);
    assert_eq!(found[0].1, vec![2, 3, 4]);
    assert_eq!(found[1].1, vec![8]);
}

#[test]
fn keywords_are_not_calls() {
    let text = "func (r *Record) Check(p string) bool {\n\tif (p == \"\") {\n\t\treturn (false)\n\t}\n\tfor (i := 0; i < 1; i++) {}\n\tswitch (p) {}\n\treturn r.ok(p)\n}\n";
    let found = calls_in("go", text, 1, "Check");
    assert_eq!(names(&found), ["ok"]);
}

#[test]
fn a_recursive_call_after_the_first_line_counts() {
    let text = "def walk(n):\n    return walk(n - 1)\n";
    let found = calls_in("python", text, 10, "walk");
    assert_eq!(names(&found), ["walk"]);
    assert_eq!(found[0].1, vec![11]);
}

#[test]
fn at_most_forty_names() {
    let text: String = (0..50).map(|i| format!("f{i}()\n")).collect();
    let found = calls_in("python", &format!("def big():\n{text}"), 1, "big");
    assert_eq!(found.len(), 40);
    assert_eq!(found[0].0, "f0");
    assert_eq!(found[39].0, "f39");
}

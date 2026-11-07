// SPDX-License-Identifier: Apache-2.0
//! Author-written tests for DCX-55b. Do not edit.
use super::names;

fn set(items: &[&str]) -> Option<std::collections::BTreeSet<String>> {
    Some(items.iter().map(|item| item.to_string()).collect())
}

#[test]
fn python_all_list_spanning_lines() {
    let text = "def alpha():\n    return 1\n\ndef beta():\n    return 2\n\nclass Gamma:\n    pass\n\n__all__ = [\n    \"beta\",\n    \"Gamma\",\n]\n";
    assert_eq!(names("python", text), set(&["Gamma", "beta"]));
}

#[test]
fn python_all_tuple_and_plus_equals() {
    let text = "__all__ = (\"a\", 'b')\n__all__ += [\"c\"]\n";
    assert_eq!(names("python", text), set(&["a", "b", "c"]));
}

#[test]
fn python_without_all_has_no_list() {
    assert_eq!(
        names("python", "def zeta():\n    return 1\nall_items = [\"x\"]\n"),
        None
    );
}

#[test]
fn commonjs_object_keys_and_assignments() {
    assert_eq!(
        names(
            "javascript",
            "function zed() {}\nfunction foo() {}\nmodule.exports = { zed, foo }\n"
        ),
        set(&["foo", "zed"])
    );
    assert_eq!(
        names("javascript", "module.exports = {\n  a: impl,\n  b,\n}\n"),
        set(&["a", "b"])
    );
    assert_eq!(
        names(
            "javascript",
            "exports.baz = function () {}\nmodule.exports.qux = 1\n"
        ),
        set(&["baz", "qux"])
    );
}

#[test]
fn es_export_list_uses_local_names_and_skips_reexports() {
    assert_eq!(
        names(
            "typescript",
            "function a1() {}\nfunction b2() {}\nfunction c3() {}\nexport { c3, b2 as main }\n"
        ),
        set(&["b2", "c3"])
    );
    assert_eq!(names("typescript", "export { x } from \"./y\"\n"), None);
}

#[test]
fn other_languages_have_no_list() {
    assert_eq!(names("rust", "pub fn a() {}\n"), None);
    assert_eq!(names("go", "func A() {}\n"), None);
}

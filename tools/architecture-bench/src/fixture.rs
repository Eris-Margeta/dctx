// SPDX-License-Identifier: Apache-2.0
//! Deterministic CC0 text fixture for the second DX-A1 corpus.
//! Implements ARCHITECTURE-PROPOSAL §7 Stage 0 scratch generation.
//! Depends on the manifest types; the measure adapter writes the bytes.

use crate::error::BenchError;
use crate::manifest::FixtureSpec;

/// One generated source file. Paths are relative and use `/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedFile {
    /// Place-relative path.
    pub path: String,
    /// UTF-8 source. Never a binary payload.
    pub bytes: String,
}

/// Builds the public-domain fixture described by the manifest.
pub fn generate(spec: &FixtureSpec) -> Result<Vec<GeneratedFile>, BenchError> {
    spec.validate()?;
    let mut files = Vec::new();
    for index in 0..spec.rust {
        files.push(rust_file(spec, index));
    }
    for index in 0..spec.javascript {
        files.push(script_file(spec, index, "js/item_", "js"));
    }
    for index in 0..spec.typescript {
        files.push(script_file(spec, index, "ts/item_", "ts"));
    }
    for index in 0..spec.markdown {
        files.push(markdown_file(spec, index));
    }
    for file in &files {
        relative_path_ok(&file.path)?;
        if file.bytes.bytes().any(|byte| byte == 0) {
            return Err(BenchError::Message(format!(
                "fixture {} contains a NUL byte",
                file.path
            )));
        }
    }
    Ok(files)
}

/// Refuses absolute paths, backslashes, and `..` components.
pub fn relative_path_ok(path: &str) -> Result<(), BenchError> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.split('/').any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(BenchError::Message(format!(
            "fixture path must stay inside the scratch place: {path}"
        )));
    }
    Ok(())
}

fn rust_file(spec: &FixtureSpec, index: u32) -> GeneratedFile {
    let name = format!("{index:04}");
    GeneratedFile {
        path: format!("src/item_{name}.rs"),
        bytes: format!(
            "//! Public-domain fixture {seed} item {name}.\n\
             //! Dedicated to the public domain under {dedication}.\n\
             //! Tokens: map index parse file node edge test symbol module.\n\
             \n\
             pub fn pd_fn_{name}(value: u32) -> u32 {{\n\
             \x20   value.wrapping_add({index})\n\
             }}\n\
             \n\
             pub struct PdType{name} {{\n\
             \x20   pub value: u32,\n\
             }}\n\
             \n\
             impl PdType{name} {{\n\
             \x20   pub const PLACE: &str = \"root\";\n\
             \x20   pub fn read(&self) -> u32 {{\n\
             \x20       self.value\n\
             \x20   }}\n\
             }}\n",
            seed = spec.seed,
            dedication = spec.dedication,
        ),
    }
}

fn script_file(spec: &FixtureSpec, index: u32, prefix: &str, extension: &str) -> GeneratedFile {
    let name = format!("{index:04}");
    let body = if extension == "ts" {
        format!(
            "/** Public-domain fixture {seed}. Dedicated under {dedication}. */\n\
             export function pdFn{name}(value: number): number {{\n\
             \x20 return value + {index};\n\
             }}\n\
             export interface PdInterface{name} {{\n\
             \x20 value: number;\n\
             }}\n",
            seed = spec.seed,
            dedication = spec.dedication,
        )
    } else {
        format!(
            "/** Public-domain fixture {seed}. Dedicated under {dedication}. */\n\
             export function pdFn{name}(value) {{\n\
             \x20 return value + {index};\n\
             }}\n\
             export class PdClass{name} {{\n\
             \x20 constructor(value) {{ this.value = value; }}\n\
             \x20 read() {{ return this.value; }}\n\
             }}\n",
            seed = spec.seed,
            dedication = spec.dedication,
        )
    };
    GeneratedFile {
        path: format!("{prefix}{name}.{extension}"),
        bytes: body,
    }
}

fn markdown_file(spec: &FixtureSpec, index: u32) -> GeneratedFile {
    let name = format!("{index:04}");
    GeneratedFile {
        path: format!("docs/item_{name}.md"),
        bytes: format!(
            "# Public domain fixture {name}\n\
             \n\
             Dedicated to the public domain under {dedication}.\n\
             Seed: {seed}.\n\
             \n\
             ## Map\n\
             \n\
             Words for retrieval: map, index, parse, file, node, edge, test, symbol, module.\n",
            dedication = spec.dedication,
            seed = spec.seed,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> FixtureSpec {
        FixtureSpec {
            id: "public-domain-fixture".into(),
            generator: "dx-a1-fixture-v1".into(),
            dedication: "CC0-1.0".into(),
            seed: "DX-A1-public-domain-fixture-v1".into(),
            size_class: "public-domain-text".into(),
            rust: 2,
            javascript: 1,
            typescript: 1,
            markdown: 1,
        }
    }

    #[test]
    fn fixture_is_deterministic_text_and_refuses_escape_paths() {
        assert!(relative_path_ok("../secret.rs").is_err());
        assert!(relative_path_ok("/tmp/x.rs").is_err());
        assert!(relative_path_ok("src/item_0000.rs").is_ok());
        let first = generate(&spec()).unwrap();
        let second = generate(&spec()).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.len(), 5);
        assert!(first.iter().all(|file| {
            !file.bytes.chars().any(|ch| ch == '\0')
                && (file.path.ends_with(".rs")
                    || file.path.ends_with(".js")
                    || file.path.ends_with(".ts")
                    || file.path.ends_with(".md"))
        }));
        assert!(first.iter().any(|file| file.bytes.contains("pd_fn_0000")));
        assert!(first.iter().any(|file| file.bytes.contains("CC0-1.0")));
        let mut empty = spec();
        empty.rust = 0;
        empty.javascript = 0;
        empty.typescript = 0;
        empty.markdown = 0;
        empty.seed.clear();
        assert!(generate(&empty).is_err());
    }
}

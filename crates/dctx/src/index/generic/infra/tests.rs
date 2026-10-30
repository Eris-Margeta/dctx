//! Tests for the infra family: eight formats, the denylist, truncated input and extension dispatch.

use super::{handles, symbols, TAGS};
use crate::index::extract::{extract_structural_refs, language_of};

/// Name and wire kind of every symbol, in source order.
fn named(tag: &str, text: &str) -> Vec<(String, String)> {
    symbols(tag, text).into_iter().map(|symbol| (symbol.name, symbol.kind)).collect()
}

/// An expected `(name, kind)` pair.
fn pair(name: &str, kind: &str) -> (String, String) {
    (name.to_string(), kind.to_string())
}

/// Start and end line of the symbol called `name`.
fn lines_of(tag: &str, text: &str, name: &str) -> [u32; 2] {
    symbols(tag, text).into_iter().find(|symbol| symbol.name == name).map(|symbol| symbol.lines).unwrap_or([0, 0])
}

const HCL: &str = "resource \"aws_s3_bucket\" \"logs\" {\n  bucket = \"x\"\n}\ndata \"aws_ami\" \"ubuntu\" {\n}\nvariable \"region\" {\n  default = \"eu\"\n}\nmodule \"vpc\" {\n  source = \"./vpc\"\n}\noutput \"id\" {\n  value = 1\n}\nprovider \"aws\" {}\n# resource \"aws_x\" \"hidden\" {}\nlocals {\n  resource = \"fake\"\n}\n";
const SQL: &str = "-- CREATE TABLE hidden (id int);\nCREATE TABLE users (\n  id INT PRIMARY KEY\n);\ncreate view active_users as select * from users;\nCREATE OR REPLACE FUNCTION bump(x int) RETURNS int AS $$ select x+1 $$ LANGUAGE sql;\nCREATE UNIQUE INDEX IF NOT EXISTS idx_users_id ON users(id);\nCREATE TRIGGER trg AFTER INSERT ON users FOR EACH ROW EXECUTE FUNCTION f();\nCREATE TYPE mood AS ENUM ('a');\nCREATE PROCEDURE \"my_proc\"() LANGUAGE sql AS $$\n  CREATE TABLE inside_body (id int);\n$$;\nCREATE TABLE public.\"Quoted\" (id int);\n/* CREATE TABLE blockhidden */\nDROP TABLE users;\n";
const SHELL: &str = "#!/bin/sh\n# helper() { not real\nsetup() {\n  echo \"${#PATH}\"\n  if [ -n \"$x\" ]; then\n    echo hi\n  fi\n}\nfunction teardown {\n  cat <<EOF\nfake() {\n}\nEOF\n}\nfunction build-all() { :; }\nrun_it ()\n{\n  echo \"x() {\"\n}\necho $(date)\ncase \"$1\" in\n  start) start_it ;;\nesac\n";
const MAKE: &str = "# build rules\nCC := cc\nOBJS = a.o b.o\n.PHONY: build test\nbuild: deps\n\tcc -o x x.c\n\t# note\n\ntest lint:\n\t./x\n%.o: %.c\n\tcc -c $<\n$(OUT): deps\nall::\n\t@true\ndefine BLOCK\ninner: not-a-target\nendef\nifeq ($(X),y)\nelse\nendif\nexport FOO := bar\nbuild:\n\t@true\n";
const DOCKER: &str = "# syntax=docker/dockerfile:1\nFROM node:20 AS builder\nRUN npm ci\n\nFROM --platform=linux/amd64 alpine as runtime\nCOPY --from=builder /a /b\nFROM scratch\nCMD [\"x\"]\n";
const PROTO: &str = "syntax = \"proto3\";\n// message Hidden {}\nmessage User {\n  string id = 1;\n  message Address {\n    string street = 1;\n  }\n  enum Kind {\n    A = 0;\n  }\n}\nservice Users {\n  rpc Get (User) returns (User);\n  rpc Watch (User) returns (stream User) {\n    option deprecated = true;\n  }\n}\nenum Top { X = 0; }\n";
const PASCAL: &str = "unit U;\ninterface\nprocedure Hello;\nfunction Twice(x: Integer): Integer;\n(* procedure Hidden; *)\n{ function Hidden2: Integer; }\n// procedure Hidden3;\ntype\n  TFoo = class\n    procedure Bar;\n  end;\nimplementation\nprocedure Hello;\nbegin\n  WriteLn('procedure Fake;');\nend;\nfunction Twice(x: Integer): Integer;\nbegin\n  Result := x * 2;\nend;\nprocedure TFoo.Bar;\nbegin\nend;\nclass function TFoo.Make: TFoo;\nbegin\nend.\n";
const GRAPHQL: &str = "\"\"\"\ntype Hidden { a: Int }\n\"\"\"\n# type Hidden2 { a: Int }\nscalar Date\ntype Query {\n  user(id: ID!): User\n  type: String\n}\ntype User implements Node & Entity {\n  id: ID!\n  \"desc\"\n  name: String\n}\ninput NewUser { name: String! }\nenum Role { ADMIN USER }\ninterface Node { id: ID! }\nunion Result = User | Query\nextend type Query { me: User }\n";

const MEMBERS_HCL: &str = "locals {\n  region = \"eu\"\n  tags = {\n    inner = 1\n  }\n  list = [\n    a = 1\n  ]\n  same == 2\n}\ndata \"aws_ami\" \"base\" {\n  most_recent = true\n}\nprovider \"aws\" {\n  region = local.region\n}\n";
const MEMBERS_SQL: &str = "CREATE SCHEMA billing;\nCREATE SCHEMA IF NOT EXISTS AUTHORIZATION bob;\nCREATE TABLE billing.invoice (\n  id INT\n);\nCREATE TYPE billing.mood AS ENUM ('sad');\nCREATE SEQUENCE invoice_seq;\nCREATE INDEX billing.idx_inv ON billing.invoice (id);\nCREATE TRIGGER audit AFTER INSERT ON billing.invoice FOR EACH ROW EXECUTE FUNCTION f();\n";
const MEMBERS_SHELL: &str = "export APP_HOME=/opt/app B=2\nalias ll=\"ls -l\"\nreadonly MAX=3\ndeclare -r LIMIT=4\ndeclare -x LEVEL=5\nexport -f start\nexport PATH\nexport APP_HOME=/again\nstart() {\n  export INSIDE=1\n}\n";
const MEMBERS_MAKE: &str = "CC := gcc\nCFLAGS ?= -O2\nLIBS += -lm\nSRCS = a.c \\\n  b.c\nbuild: CFLAGS = -g\n\t$(CC) x.c\noverride X = 1\nCC := clang\n";
const MEMBERS_DOCKER: &str = "ARG BASE=node\nFROM node:20 AS base\nARG VERSION=1\nENV NODE_ENV=production PORT=\"80 81\" \\\n  DEBUG=0\nENV legacy value\nFROM alpine\nENV OTHER=1\n";
const MEMBERS_PROTO: &str = "message User {\n  string id = 1;\n  repeated string tags = 2;\n  map<string, int32> counts = 3;\n  oneof kind {\n    int32 a = 4;\n  }\n  option deprecated = true;\n  reserved 9;\n  enum Role {\n    option allow_alias = true;\n    ADMIN = 0;\n    GUEST = -1;\n  }\n}\nservice S {\n  option x = 1;\n  rpc Get (User) returns (User);\n}\n";
const MEMBERS_GRAPHQL: &str = "type User {\n  id: ID!\n  posts(\n    first: Int\n    after: String\n  ): [Post]\n  \"\"\"\n  fake: Int\n  \"\"\"\n  name: String\n}\ninput NewUser {\n  name: String\n}\nenum Role {\n  ADMIN\n  USER @deprecated\n}\n";

#[test]
fn terraform_locals_keys_and_data_blocks_are_symbols() {
    assert_eq!(
        named("hcl", MEMBERS_HCL),
        [
            pair("region", "const"),
            pair("tags", "const"),
            pair("list", "const"),
            pair("data.aws_ami.base", "type"),
            pair("aws_ami.base", "type"),
            pair("aws", "type")
        ]
    );
    assert_eq!(lines_of("hcl", MEMBERS_HCL, "tags"), [3, 5]);
}

#[test]
fn sql_schemas_sequences_and_qualified_names_use_the_last_segment() {
    assert_eq!(
        named("sql", MEMBERS_SQL),
        [
            pair("billing", "type"),
            pair("invoice", "type"),
            pair("mood", "type"),
            pair("invoice_seq", "type"),
            pair("idx_inv", "type"),
            pair("audit", "fn")
        ]
    );
}

#[test]
fn shell_variables_and_aliases_are_symbols_at_file_level() {
    assert_eq!(
        named("shell", MEMBERS_SHELL),
        [
            pair("APP_HOME", "var"),
            pair("B", "var"),
            pair("ll", "fn"),
            pair("MAX", "const"),
            pair("LIMIT", "const"),
            pair("LEVEL", "var"),
            pair("start", "fn")
        ]
    );
}

#[test]
fn makefile_variable_assignments_are_symbols() {
    assert_eq!(named("make", MEMBERS_MAKE), [pair("CC", "var"), pair("CFLAGS", "var"), pair("LIBS", "var"), pair("SRCS", "var"), pair("build", "fn"), pair("X", "var")]);
    assert_eq!(lines_of("make", MEMBERS_MAKE, "SRCS"), [4, 5]);
}

#[test]
fn dockerfile_arg_and_env_names_sit_under_their_stage() {
    assert_eq!(
        named("docker", MEMBERS_DOCKER),
        [
            pair("BASE", "var"),
            pair("base", "type"),
            pair("base::VERSION", "var"),
            pair("base::NODE_ENV", "var"),
            pair("base::PORT", "var"),
            pair("base::DEBUG", "var"),
            pair("base::legacy", "var"),
            pair("OTHER", "var")
        ]
    );
}

#[test]
fn protobuf_fields_and_enum_values_nest_under_their_message() {
    assert_eq!(
        named("proto", MEMBERS_PROTO),
        [
            pair("User", "type"),
            pair("User::id", "field"),
            pair("User::tags", "field"),
            pair("User::counts", "field"),
            pair("User::a", "field"),
            pair("User::Role", "type"),
            pair("User::Role::ADMIN", "const"),
            pair("User::Role::GUEST", "const"),
            pair("S", "type"),
            pair("S::Get", "fn")
        ]
    );
}

#[test]
fn graphql_fields_and_enum_values_nest_under_their_type() {
    assert_eq!(
        named("graphql", MEMBERS_GRAPHQL),
        [
            pair("User", "type"),
            pair("User::id", "field"),
            pair("User::posts", "field"),
            pair("User::name", "field"),
            pair("NewUser", "type"),
            pair("NewUser::name", "field"),
            pair("Role", "type"),
            pair("Role::ADMIN", "const"),
            pair("Role::USER", "const")
        ]
    );
}

#[test]
fn every_infra_format_finds_its_declarations() {
    assert_eq!(
        named("hcl", HCL),
        [
            pair("aws_s3_bucket.logs", "type"),
            pair("data.aws_ami.ubuntu", "type"),
            pair("aws_ami.ubuntu", "type"),
            pair("region", "type"),
            pair("vpc", "type"),
            pair("id", "type"),
            pair("aws", "type"),
            pair("resource", "const")
        ]
    );
    assert_eq!(
        named("sql", SQL),
        [
            pair("users", "type"),
            pair("active_users", "type"),
            pair("bump", "fn"),
            pair("idx_users_id", "type"),
            pair("trg", "fn"),
            pair("mood", "type"),
            pair("my_proc", "fn"),
            pair("Quoted", "type")
        ]
    );
    assert_eq!(named("shell", SHELL), [pair("setup", "fn"), pair("teardown", "fn"), pair("build-all", "fn"), pair("run_it", "fn")]);
    assert_eq!(
        named("make", MAKE),
        [pair("CC", "var"), pair("OBJS", "var"), pair("build", "fn"), pair("test", "fn"), pair("lint", "fn"), pair("all", "fn"), pair("FOO", "var")]
    );
    assert_eq!(named("docker", DOCKER), [pair("builder", "type"), pair("runtime", "type")]);
    assert_eq!(
        named("proto", PROTO),
        [
            pair("User", "type"),
            pair("User::id", "field"),
            pair("User::Address", "type"),
            pair("User::Address::street", "field"),
            pair("User::Kind", "type"),
            pair("User::Kind::A", "const"),
            pair("Users", "type"),
            pair("Users::Get", "fn"),
            pair("Users::Watch", "fn"),
            pair("Top", "type")
        ]
    );
    assert_eq!(
        named("pascal", PASCAL),
        [pair("Hello", "fn"), pair("Twice", "fn"), pair("Bar", "fn"), pair("TFoo.Bar", "fn"), pair("TFoo.Make", "fn")]
    );
    assert_eq!(
        named("graphql", GRAPHQL),
        [
            pair("Date", "type"),
            pair("Query", "type"),
            pair("Query::user", "field"),
            pair("Query::type", "field"),
            pair("User", "type"),
            pair("User::id", "field"),
            pair("User::name", "field"),
            pair("NewUser", "type"),
            pair("Role", "type"),
            pair("Node", "type"),
            pair("Result", "type")
        ]
    );
}

#[test]
fn end_lines_follow_braces_dollar_bodies_recipes_and_stages() {
    assert_eq!(lines_of("hcl", HCL, "aws_s3_bucket.logs"), [1, 3]);
    assert_eq!(lines_of("sql", SQL, "users"), [2, 4]);
    assert_eq!(lines_of("sql", SQL, "my_proc"), [10, 12]);
    assert_eq!(lines_of("shell", SHELL, "setup"), [3, 8]);
    assert_eq!(lines_of("shell", SHELL, "teardown"), [9, 14]);
    assert_eq!(lines_of("shell", SHELL, "run_it"), [16, 19]);
    assert_eq!(lines_of("make", MAKE, "build"), [5, 7]);
    assert_eq!(lines_of("make", MAKE, "lint"), [9, 10]);
    assert_eq!(lines_of("docker", DOCKER, "builder"), [2, 3]);
    assert_eq!(lines_of("docker", DOCKER, "runtime"), [5, 6]);
    assert_eq!(lines_of("proto", PROTO, "User"), [3, 11]);
    assert_eq!(lines_of("proto", PROTO, "Users::Watch"), [14, 16]);
    assert_eq!(lines_of("graphql", GRAPHQL, "Query"), [6, 9]);
    assert_eq!(lines_of("pascal", PASCAL, "Hello"), [3, 3]);
}

#[test]
fn symbols_carry_the_signature_and_the_sig_summary() {
    let found = symbols("hcl", HCL);
    assert_eq!(found[0].signature, "resource \"aws_s3_bucket\" \"logs\"");
    assert_eq!(found[0].summary, found[0].signature);
    assert_eq!(found[0].summary_source, "sig");
}

#[test]
fn denylisted_keywords_are_never_symbols() {
    for word in ["if", "else", "for", "while", "match", "switch", "return", "case", "catch", "when", "do", "try"] {
        for (tag, text) in [
            ("shell", format!("{word}() {{\n}}\nfunction {word} {{\n}}\n")),
            ("pascal", format!("procedure {word};\nfunction {word}: Integer;\n")),
            ("proto", format!("message {word} {{\n}}\nenum {word} {{}}\n")),
            ("graphql", format!("type {word} {{\n}}\n")),
            ("make", format!("{word}:\n\t@true\n")),
            ("docker", format!("FROM x AS {word}\n")),
            ("sql", format!("CREATE TABLE {word} (id int);\nCREATE FUNCTION {word}() RETURNS int;\n")),
            ("hcl", format!("variable \"{word}\" {{}}\nmodule \"{word}\" {{}}\n")),
        ] {
            let found = named(tag, &text);
            assert!(found.is_empty(), "{tag} {word}: {found:?}");
        }
    }
}

#[test]
fn malformed_and_truncated_input_never_panics() {
    let samples = [
        "", "resource \"a", "resource \"a\" \"b", "variable \"\" {", "CREATE", "CREATE TABLE", "create or replace", "CREATE OR REPLACE FUNCTION f(",
        "$$ $$ $$", "function", "function ", "f(", "f()", "<<", "<<EOF", "<<-'", "FROM", "FROM x AS", "message", "message {",
        "rpc", "}}}} message A {{ rpc b(", "\"\"\" unterminated", "/* unterminated", "(* unterminated", "{ unterminated",
        "procedure", "class", "procedure .", "procedure 1x;", "x:", ":", "::=", "define", "define X\n", "a b c: \n\t",
        "locals {", "locals {\n  a =", "locals {\n  a = [\n", "export", "export A=", "alias =", "alias", "declare -r", "ARG", "ENV", "ENV A=\"", "ENV A=1 \\\n", "CC :=", "export X ?=", "X = a \\\n",
        "enum E {\n A = ", "type T {\n a(", "type T {\n a(\n", "message M {\noneof ", "message M {\noneof x {\n int32 a = 1;\n}\n", "CREATE SCHEMA", "CREATE SEQUENCE", "CREATE SCHEMA AUTHORIZATION",
        "\u{1F600}: x", "r\u{e9}sum\u{e9}() {", "\r\n\r\n", "\u{0}\u{1}", "type", "type X", "scalar\n",
    ];
    for tag in TAGS {
        for sample in samples {
            let _ = symbols(tag, sample);
        }
        let deep = format!("message D {} {}", "{".repeat(5000), "}".repeat(10));
        let _ = symbols(tag, &deep);
        assert!(symbols(tag, "").is_empty());
    }
    for (tag, full) in [("hcl", HCL), ("sql", SQL), ("shell", SHELL), ("make", MAKE), ("docker", DOCKER), ("proto", PROTO), ("pascal", PASCAL), ("graphql", GRAPHQL), ("hcl", MEMBERS_HCL), ("sql", MEMBERS_SQL), ("shell", MEMBERS_SHELL), ("make", MEMBERS_MAKE), ("docker", MEMBERS_DOCKER), ("proto", MEMBERS_PROTO), ("graphql", MEMBERS_GRAPHQL)] {
        for cut in (0..full.len()).filter(|&at| full.is_char_boundary(at)) {
            let _ = symbols(tag, &full[..cut]);
        }
    }
    assert!(symbols("unknown", "message A {}").is_empty());
}

#[test]
fn extensions_and_basenames_dispatch_to_the_infra_family() {
    for (path, tag) in [
        ("main.tf", "hcl"),
        ("prod.tfvars", "hcl"),
        ("a.hcl", "hcl"),
        ("s.sql", "sql"),
        ("b.sh", "shell"),
        ("b.bash", "shell"),
        ("b.zsh", "shell"),
        ("rules.mk", "make"),
        ("Makefile", "make"),
        ("GNUmakefile", "make"),
        ("sub/makefile", "make"),
        ("Dockerfile", "docker"),
        ("ci/Containerfile", "docker"),
        ("p.proto", "proto"),
        ("u.pas", "pascal"),
        ("u.pp", "pascal"),
        ("u.dpr", "pascal"),
        ("q.graphql", "graphql"),
        ("q.gql", "graphql"),
        ("S.SQL", "sql"),
    ] {
        assert_eq!(language_of(path), tag, "{path}");
        assert!(handles(tag));
        assert!(super::super::handles(tag));
    }
    assert_eq!(language_of("Makefile.bak"), "opaque");
    assert!(!handles("python") && !handles("scala") && !handles("opaque"));
    let (found, refs) = extract_structural_refs("Makefile", b"build: deps\n\tcc -o x x.c\n");
    assert_eq!(found.iter().map(|symbol| symbol.name.as_str()).collect::<Vec<_>>(), ["build"]);
    assert!(refs.is_empty());
    assert!(extract_structural_refs("a.sh", &[0xff]).0.is_empty());
}

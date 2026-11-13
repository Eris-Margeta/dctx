const QUERY: &str = r#"
fn phantom_raw() {}
"nested quotes" // data
"#;
const TEMPLATE: &str = r###"
struct PhantomRaw;
"## not a terminator
"###;
fn execute() {}

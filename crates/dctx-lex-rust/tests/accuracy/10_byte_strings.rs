static HEADER: &[u8] = b"
fn phantom_bytes() {}
";
const RAW: &[u8] = br##"
fn phantom_raw_bytes() {}
"##;
fn send() { let _ = b'f'; }

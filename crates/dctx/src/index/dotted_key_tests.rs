//! Author-written tests for DCX-56b. Do not edit.
use super::{defines, reads, segments};

#[test]
fn dotted_queries_split_into_segments() {
    assert_eq!(segments("server.port"), Some(vec!["server", "port"]));
    assert_eq!(segments("logging.level.root"), Some(vec!["logging", "level", "root"]));
    assert_eq!(segments("my-app.max_conn2"), Some(vec!["my-app", "max_conn2"]));
}

#[test]
fn other_queries_are_not_dotted_keys() {
    for query in ["port", "server.", ".port", "server..port", "server port", "server.port extra", "1abc.port", "a.b(c)", "HOME"] {
        assert_eq!(segments(query), None, "{query}");
    }
}

#[test]
fn a_config_symbol_defines_the_key() {
    assert!(defines("server::port", &["server", "port"]));
    assert!(defines("server.port", &["server", "port"]));
    assert!(!defines("server::host", &["server", "port"]));
    assert!(!defines("port", &["server", "port"]));
    assert!(!defines("app::server::port", &["server", "port"]));
    assert!(!defines("Server::port", &["server", "port"]));
}

#[test]
fn code_lines_that_read_the_key() {
    let key = ["server", "port"];
    assert!(reads("    port = cfg[\"server\"][\"port\"]", &key));
    assert!(reads("    port = cfg['server'] ['port']", &key));
    assert!(reads("\treturn v.GetString(\"server.port\")", &key));
    assert!(reads("level = cfg.get('server.port')", &key));
    assert!(reads("p = settings.server.port", &key));
}

#[test]
fn lines_that_do_not_read_the_key() {
    let key = ["server", "port"];
    assert!(!reads("p = settings.server.portal", &key));
    assert!(!reads("p = cfg[\"server\"]", &key));
    assert!(!reads("x = \"server.port.extra\"", &key));
    assert!(!reads("# server port", &key));
    assert!(!reads("p = cfg[\"port\"][\"server\"]", &key));
}

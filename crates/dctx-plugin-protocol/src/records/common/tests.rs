// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::json::decode::{DecodeLimits, decode_object};

fn json(text: &str) -> Json {
    decode_object(text.as_bytes(), DecodeLimits::WIRE).expect("json")
}

#[test]
fn version_constant_is_exactly_one_zero() {
    assert_eq!(ApiVersion::decode(&Json::Array(vec![Json::Int(1), Json::Int(0)])), Ok(ApiVersion::V1_0));
    assert!(ApiVersion::decode(&Json::Array(vec![Json::Int(1), Json::Int(1)])).is_err());
    assert!(ApiVersion::decode(&Json::Array(vec![Json::Int(1)])).is_err());
}

#[test]
fn limits_bounds_are_enforced() {
    let text = r#"{"frame_bytes":1048576,"read_bytes":8388608,"actions":128,"outstanding_requests":16,"diagnostics_bytes":65536,"chunk_bytes":65536,"state_bytes":65536,"upload_bytes":1052672,"queue_bytes":2097152,"memory_bytes":67108864,"work_units":1000000}"#;
    let limits = Limits::decode(&json(text)).expect("valid");
    assert_eq!(Limits::decode(&limits.encode()), Ok(limits));
    assert!(Limits::decode(&json(&text.replace("1048576", "1048577"))).is_err());
    assert!(Limits::decode(&json(&text.replace(":16,", ":17,"))).is_err());
    assert!(Limits::decode(&json(&text.replace(",\"work_units\":1000000", ""))).is_err());
}

#[test]
fn waits_resources_and_records() {
    assert!(matches!(Wait::decode(&json(r#"{"broker_request":"p:1"}"#)), Ok(Wait::BrokerRequest(_))));
    assert!(matches!(Wait::decode(&json(r#"{"dependency":"dep:1"}"#)), Ok(Wait::Dependency(_))));
    assert!(Wait::decode(&json(r#"{"broker_request":"p:1","dependency":"d"}"#)).is_err());
    assert!(Wait::decode(&json(r"{}")).is_err());
    let resource = Resource::decode(&json(r#"{"handle":"file:ignore","kind":"file","bytes":3}"#)).expect("valid");
    assert_eq!(Resource::decode(&resource.encode()), Ok(resource));
    let record = CommandRecord::decode(&json(r#"{"type":"record","name":"protection","fields":{"protected":true,"n":[1,"x"]}}"#))
        .expect("valid");
    assert_eq!(CommandRecord::decode(&record.encode()), Ok(record));
    assert!(CommandRecord::decode(&json(r#"{"type":"blob","text":"x"}"#)).is_err());
    assert!(decode_config(&json(r#"{"a":[[1]]}"#)).is_err());
    assert!(decode_config(&json(r#"{"Bad":1}"#)).is_err());
    assert!(decode_extensions(&json(r#"{"x":{"y":1}}"#)).is_err());
    assert!(decode_hook_decl(&json(r#"{"name":"cache-gc","version":2}"#)).is_err());
}

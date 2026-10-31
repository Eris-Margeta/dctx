# Card 2: wire or delete DCX-55c and DCX-56c

Goal: no dead library code. Files: `map-query/src/order.rs` → `cli/src/cmd/map.rs`; `map-query/src/find.rs` → `cli/src/cmd/find.rs`. Gates: existing unit tests plus one CLI snapshot each (`map` ordering on `pocketbase` export list; `find database.host` returns the TOML/YAML key). If a worker can't wire in one card, delete the module and note it in CHANGELOG.

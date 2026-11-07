<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Rust library example

Pack an in-memory source through the `dctx` facade, with explicit policy, clock,
cancellation and an 8 MiB ephemeral storage ceiling. No home directory, disk cache
or plugin host is discovered. The fixed clock is suitable for this synthetic source;
a filesystem host should supply its own live clock and selection rules.

<!-- library-example:begin -->
```rust
use dctx::{memory::{CancelFlag, FixedClock, MemorySource}, *};
let mut source = MemorySource::new();
source.insert_file("hello.rs", b"pub fn hello() {}\n", 0)?;
let mut session = Session::builder(source).clock(FixedClock(Timestamp(0)))
    .cancellation(std::sync::Arc::new(CancelFlag::new())).ceiling(HostCeiling::base())
    .selection(SelectionPolicy::from_texts(&RuleTexts::default())).grants(HostGrants::PermitAll)
    .storage(StoragePolicy::Ephemeral { max_bytes: 8 * 1024 * 1024 }).place("demo").view("pack").build()?;
let mut output = VecSink::new();
session.pack(&PackRequest::new(), &mut output)?;
println!("{}", std::str::from_utf8(output.as_bytes())?);
# Ok::<(), Box<dyn std::error::Error>>(())
```
<!-- library-example:end -->

Rustdoc supplies `main` and hides the final error-type annotation. Test the actual
snippet from the repository root:

```sh
cargo test --locked -p dctx-library-example --doc
```

The workspace CI test job includes this doctest. Its local path dependency tests
the maintained source; downstream consumers use the tagged Git dependency shown
in the [main README](../../README.md#use-as-a-library) until crates.io publication.

# Dependency maintenance

Removed the unused `tempfile` dev dependency from `dctx-pack`. Tests in
`dctx-adapter-store` and `dctx-lex-kt` use their existing normal dependencies on
`dctx-map-segment` and `dctx-map-model`, respectively. The library example keeps
`dctx` as a dev dependency for its doctest.

## Remaining duplicates

Reproduce the default graph with `cargo tree -d --locked` and include optional
features and platform-specific edges with
`cargo tree -d --workspace --all-features --target all --locked`.
Use `cargo tree --workspace --all-features --target all --locked -i NAME@VERSION`
to inspect the callers for a row.

| Package | Resolved versions | Reason for retaining the versions |
| --- | --- | --- |
| `base64` | 0.21.7, 0.22.1, 0.23.1 | Swift bindings, Wry, and the HTTP/plist/Tauri code generation stack require different releases. |
| `bit-set` | 0.8.0, 0.11.1 | Tauri's `dom_query` and the workspace's `proptest` use different releases. |
| `bit-vec` | 0.8.0, 0.10.1 | The two `bit-set` versions and `proptest` require these releases. |
| `bitflags` | 1.3.2, 2.13.2 | WebKitGTK and `png` 0.17 use 1.x. Rustix, SQLite, and newer platform bindings use 2.x. |
| `cpufeatures` | 0.2.17, 0.3.1 | SHA-2 and Curve25519 use 0.2. Proptest's RNG chain uses ChaCha20 with 0.3. |
| `dirs` | 5.0.1, 7.0.0 | The desktop application's manifest uses 5.x. Tauri and Wry use 7.x. |
| `dirs-sys` | 0.4.1, 0.5.0 | The two `dirs` major versions use different platform backends. |
| `foldhash` | 0.1.5, 0.2.0 | Petgraph's `hashbrown` uses 0.1. Tauri's `dom_query` and newer `hashbrown` use 0.2. |
| `getrandom` | 0.2.17, 0.3.4, 0.4.3 | Redox user support, Tauri/`tempfile`, and UUID/`rand` require separate releases. |
| `hashbrown` | 0.12.3, 0.15.5, 0.17.1 | Indexmap 1.x, Petgraph, and Indexmap 2.x/SQLite's `hashlink` require separate releases. |
| `heck` | 0.4.1, 0.5.0 | GLib's macros use 0.4. System dependency discovery and Tauri's macros use 0.5. |
| `indexmap` | 1.9.3, 2.14.2 | Tauri's schema build enables Indexmap 1.x through Schemars. TOML tooling and Petgraph use 2.x. |
| `jni-sys` | 0.3.1, 0.4.1 | Android's JNI and NDK crates require the 0.3 compatibility release, which depends on 0.4. |
| `miniz_oxide` | 0.8.9, 0.9.1 | Both PNG releases use 0.8. `flate2` uses 0.9. |
| `png` | 0.17.16, 0.18.1 | Tauri's ICO decoder uses 0.17. Image, Muda, and Tauri code generation use 0.18. |
| `proc-macro-crate` | 1.3.1, 2.0.2, 3.5.0 | GTK3, GLib, and NumEnum macros require different major releases. |
| `quick-error` | 1.2.3, 2.0.1 | Proptest's `rusty-fork` uses 1.x. The desktop image stack's TIFF decoder uses 2.x. |
| `quick-xml` | 0.41.0, 0.42.0 | The Wayland protocol scanner and plist parser use different releases. |
| `r-efi` | 5.3.0, 6.0.0 | `getrandom` 0.3 and 0.4 use different UEFI bindings. |
| `redox_users` | 0.4.6, 0.5.3 | The two `dirs-sys` releases select different Redox user libraries. |
| `serde_spanned` | 0.6.9, 1.1.2 | The old and new TOML stacks require different releases. |
| `syn` | 1.0.109, 2.0.119, 3.0.9 | GTK/GLib macro infrastructure still brings in 1.x. Most derive macros use 2.x. `dctx-lex-rust` uses 3.x for parsing. |
| `thiserror`, `thiserror-impl` | 1.0.69, 2.0.18 | GTK/GLib and older platform dependencies use 1.x. The dctx crates and newer desktop dependencies use 2.x. Each release selects its matching derive crate. |
| `toml` | 0.8.2, 1.1.8+spec-1.1.0 | GTK's `system-deps` uses 0.8. Tauri and its build tools use 1.x. |
| `toml_datetime` | 0.6.3, 1.1.2+spec-1.1.0 | The older TOML editors and TOML 1.x require different releases. |
| `toml_edit` | 0.19.15, 0.20.2, 0.25.17+spec-1.1.0 | The GTK3 and NumEnum macro stacks use different releases. TOML 0.8 selects 0.20. |
| `windows-sys` | 0.45.0, 0.48.0, 0.60.2, 0.61.2 | JNI, desktop directory lookup, Tauri shell/dialog dependencies, and dctx/Rustix use different Windows binding releases. |
| `windows-targets` | 0.42.2, 0.48.5, 0.53.5 | The three older `windows-sys` releases select separate target libraries. |
| `windows_aarch64_gnullvm`, `windows_aarch64_msvc`, `windows_i686_gnu`, `windows_i686_msvc`, `windows_x86_64_gnu`, `windows_x86_64_gnullvm`, `windows_x86_64_msvc` | 0.42.2, 0.48.5, 0.53.1 | Each `windows-targets` release selects its corresponding architecture libraries. |
| `winnow` | 0.5.40, 1.0.4 | The older TOML editors and the TOML 1.x parser use different releases. |

The two tree commands also list the same version of `fastrand`, `objc2`,
`objc2-foundation`, `percent-encoding`, `proc-macro2`, `quote`, `schemars`,
`semver`, `serde`, `serde_core`, `serde_json`, `smallvec`, `swift-rs`,
`tauri-utils`, `time`, `uuid`, and `wasm-bindgen` more than once. Cargo
separates dependency instances by build context, target, or enabled features.
For example, Tauri's build tools enable different Schemars features from the
dctx library's RPC schema code.
`Cargo.lock` also retains `rand_core` 0.6.4 for optional RNG support in
`signature` alongside active 0.10.1, and `schemars` 0.9.0 and 1.2.3 for
optional `serde_with` integrations alongside active 0.8.22.

## RustSec audit

On 2026-11-18, `cargo audit` 0.22.2 scanned 593 locked dependencies using
RustSec advisory database revision `472a4c9a7`. It exited successfully with
zero vulnerability errors and four warnings.

| Package | Warning | Affected dependency or code path |
| --- | --- | --- |
| `chrono` 0.4.45 | Unmaintained | The desktop app uses it for recent-project timestamps. [RUSTSEC-2026-0543](https://rustsec.org/advisories/RUSTSEC-2026-0543) |
| `proc-macro-error` 1.0.4 | Unmaintained | The desktop GTK/GLib macro stack uses it at build time. [RUSTSEC-2024-0370](https://rustsec.org/advisories/RUSTSEC-2024-0370) |
| `glib` 0.18.5 | Unsound | The desktop GTK stack brings in the affected `VariantStrIter` implementation. [RUSTSEC-2024-0429](https://rustsec.org/advisories/RUSTSEC-2024-0429) |
| `rustix` 1.1.5 | Unsound | The workspace and desktop dependency chains use Rustix. The advisory concerns extended-attribute calls with zero-length buffers. [RUSTSEC-2026-0486](https://rustsec.org/advisories/RUSTSEC-2026-0486) |

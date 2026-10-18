# Plugins

A plugin adds commands or hooks through a bounded, versioned JSON protocol over
stdio. The host brokers its requested effects; installation, enablement,
capability consent and execution admission are separate steps. Bundled code can
run as a worker of the same executable; independently installed artifacts use
the plugin protocol. Availability is limited as described below.
dctx, the bundled Git plugin and this guide are Apache-2.0 (decision 078).

## Bundled Git plugin and consent

The default CLI includes `dctx/git` version `1.0.0`, with alias `dctx git` and
the verbs `status` and `protect`. The catalog defaults to enabled. A fresh CLI
without a consent record reports `enabled` with `needs-consent`; shipping the
worker does not grant its capabilities or guarantee that it can launch.
`dctx git` is a plugin alias, not a general Git passthrough.

The [bundled manifest capabilities](../../crates/dctx-cli/src/plugin/bundled.rs)
disclose these grants:

| Capability | Bound |
| --- | --- |
| Required `file.read` | Project `.gitignore`, at most 1,048,576 bytes. |
| Required `file.write` | Existing project `.gitignore` only (`create=false`); growth at most 4,096 bytes, total at most 1,052,672 bytes. |
| Optional `program.run` | Program `git`, operation `git.add-ignore`, resource `git-admin:stage-ignore`, output at most 65,536 bytes. |
| Outputs / network | Neither requested. |

This narrow optional Git recipe belongs to the plugin boundary. The map/core
does not run Git, read `.git/` or use Git hashes for identity (decision 078).

Review the requested capabilities before approving:

```sh
dctx plugin list
dctx plugin info dctx/git --json
dctx plugin enable dctx/git --approve
dctx plugin disable dctx/git
```

`--approve` records consent for the displayed capability set. New installations
remain inactive until enabled; combined `install --enable` requires `--approve`.
Capability expansion can require fresh consent. Explicit disable persists
across updates. `enable --project PATH` scopes consent to a project; omitting it
uses user scope. `disable --project PATH` is accepted/reported, but the current
CLI disable operation is user-wide.

The desktop presents these states:

| State | Meaning |
| --- | --- |
| `enabled` | Enabled in the registry; execution still requires admission. |
| `needs-consent` | Review and approve the requested capabilities. |
| `disabled` | Disabled by the user. |
| `failed` | A failure is reported; inspect the last failure message. |

The CLI additionally separates enablement (`inactive`, `enabled`, `disabled`)
from consent (`inactive`, `needs-consent`, `granted`, `disabled`) and reports
worker availability. A strict build lists the bundled catalog as disabled,
with consent `unavailable` and worker `excluded`.

## CLI reference

These are the command shapes from the plugin help renderer
([`render.rs`](../../crates/dctx-cli/src/plugin/render.rs)) and
[`parse.rs`](../../crates/dctx-cli/src/plugin/parse.rs):

```text
dctx plugin search [query] [--refresh] [--json]
dctx plugin install <owner/repo>[@version] [--enable --approve] [--json]
dctx plugin install --url <https-url> --id <publisher/name> [--json]
dctx plugin install --file <bundle> --id <publisher/name> [--json]
dctx plugin list [--json]
dctx plugin info <id> [--json]
dctx plugin enable <id> [--project <path>] [--approve] [--json]
dctx plugin disable <id> [--project <path>] [--json]
dctx plugin remove <id> [--json]
dctx plugin update [<id>] [--version <version>] [--check] [--json]
dctx plugin verify [<id>] [--refresh-trust] [--json]
dctx plugin run <id> -- <args>
dctx git <status|protect>
```

`dctx plugin`, `dctx plugin help`, `--help` and `-h` show plugin help.
Search accepts one query argument; quote phrases. Search and list use cached
metadata; `search --refresh` requests a fetch. List and help start no worker,
fetch nothing and do not create a project `.dctx/` directory.

Install accepts exactly one source: repository locator, HTTPS manifest URL or
offline bundle. URL/file sources require `--id`; the parser also accepts
`--enable --approve` with those sources. `update --check` reports a candidate
without downloading; omitting the ID in update/verify selects the bundled plugin.
`verify --refresh-trust` requests a revocation freshness check. Bundled updates
come with the containing CLI/app release. `remove` masks the bundled copy when
applicable. The current product's offline limits still apply to these commands.

`run` requires the `--` separator. The Git alias rejects that separator.
The parser recognizes `--json` for plugin and alias requests, including `run`,
and strips it before forwarding plugin arguments.

## Desktop: Settings > Plugins

Open **Settings > Plugins** to inspect bundled/installed source, version,
state, requested capabilities and the last failure. **Refresh plugins** reloads
the list. **Enable** and **Disable** call the host management API.
**Grant consent** opens a review dialog with plugin ID, version, artifact
SHA-256, capability descriptions and exact grants/bounds. **Confirm grant**
records user-wide consent for all projects; **Cancel** closes the dialog.
**Check for update** reports cached information and explicitly states that
online discovery is unavailable. These behaviors are implemented in
[`plugins.ts`](../../apps/dctx-desktop/src/plugins.ts); opening settings does
not itself execute plugin code or fetch from a network.

## Strict build

The [`dctx` library](../../crates/dctx/Cargo.toml) includes the optional
`plugin-runtime` feature in its defaults. Hosts requiring the engine without
that runtime must disable defaults and select `pack`, `map`, `templates`, `fs`,
`segments`, `cache` as needed. The runtime adds the host, platform and protocol
dependencies; sessions still require an explicit host.

For the [CLI](../../crates/dctx-cli/Cargo.toml), select
`--no-default-features --features strict` when building. Cargo features are
additive: selecting `strict` alone leaves defaults enabled. This composition
excludes `dctx-plugin-host`, `dctx-plugin-platform` and `dctx-plugin-git` from
normal dependencies. Management metadata/protocol/distribution code remains;
strict does not mean that every crate whose name contains `plugin` disappears.

## Trust and signatures

Distribution verifies detached Ed25519 signatures over domain-separated exact
manifest/index bytes, checks SHA-256 artifact integrity, and uses pinned
publisher keys, revocation freshness and rollback checks. A key downloaded
beside an artifact is not an independent trust anchor. Verification does not
grant capabilities or establish confinement. The production verifier never
signs or generates keys; signing belongs only in separate release tooling
(test fixtures use test keys). **Unverified:** a completed production release
signing tool in this checkout; the design describes it as separate work.
The product CLI currently supplies no index key and no TLS network adapter.

## What is not yet available

- Qualified native plugin launch on any OS. Linux has restricted-process
  building blocks, but arbitrary third-party ELF launch remains unsupported;
  macOS lacks the reviewed Seatbelt/initial-exec composition; Windows and other
  targets are unsupported. The platform provider also refuses bundled launch
  and the Git recipe. A consent record cannot bypass these admission failures.
- Online discovery, downloads and updates in the current product wiring.
  Search can inspect cached metadata and the distribution engine supports
  verified offline bundles, but network refresh needs a configured trust key
  and network adapter. The desktop explicitly reports offline update discovery.
- Public plugin publication through this CLI. There is no `plugin publish`
  command. The default discovery URL is `https://example.invalid/dctx/index.json`;
  a live public registry/release publication is **unverified**. Local index
  tooling and signed fixture tests do not establish public availability.

Sources: [plugin design](../design/PLUGINS.md),
[native platform limits](../../crates/dctx-plugin-platform/README.md),
[Ed25519 verification](../../crates/dctx-plugin-distribution/src/domain/signature.rs),
and the [crate map](../CRATE-MAP.md).

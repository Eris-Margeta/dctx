# Contract corpus

PL-C · data for PL-P/PL-D/PL-M/PL-H/PL-G/PL-I conformance.
Depends on the adjacent schemas and normative semantic documents.
Nothing here executes a plugin, signs a release or establishes publisher trust.

`catalog.json` is the authoritative case inventory. IDs are stable predicate
inputs. Paths are relative to the plugin-api directory. JSON fixtures preserve
their exact file bytes; do not normalize a signed/digested document on load.
The same input may be valid structurally and refused semantically or by state.

| Catalog field | Meaning |
|---|---|
| `id`, `path` | Stable case ID and actual input file |
| `schema` | Local registered schema URI, when applicable |
| `expect.valid` | Acceptance of the input at its stated validation layer |
| `expect.stage` | `frame`, `decode`, `schema`, `semantic`, `artifact`, `session`, `dts-source`, `scenario` |
| `expect.code` | Required typed refusal, when one is applicable |
| `expect.predicate` | Named rule owned by the requirements matrix |
| `context` | Pending method, direction, prior state/sequence or other injected facts |
| `positive_control` | Companion accepted input or baseline scenario |
| `expect.outcome` | Scenario observations, replacing a boolean accept/reject claim |

For frame cases decode the descriptor's exact hex bytes (not the JSON descriptor
as a protocol frame). `negotiated=false` selects the bootstrap cap. All other
wire JSON/text fixtures are **bodies**: frame their exact bytes using u32 LE
when exercising the stream codec. Decode-stage cases deliberately contain
invalid JSON grammar/lexemes, duplicate keys, surrogates or excessive nesting/
members; normal JSON loading before the tested decoder would destroy evidence.
Feed valid frames at every prefix/body split and with back-to-back frames.

Schema-stage positives prove shape only. Session-stage cases inject the named
state and pending request; positive bootstrap references use the supplied hello
and fixture install identity. `hello-error` instead uses its explicitly
incompatible host offer. Broker shape cases do not assert that the Git manifest
requests state/output grants. `manifest-observer` declares all lifecycle hooks
and state/output requests for generic fake-host tests. Hook shapes other than
the Git hook need an appropriate fake declared manifest/ceiling in integration.
Semantic tests resolve declarations against manifest-git unless a case supplies
another manifest/context; do not silently grant authority to make a test pass.
Artifact failures use the decoded `abc.bin` bytes in bytes/nist.json and must
fail size/hash checks **before** any attempted executable validation/launch.

The two-plugin DTS cycle case tests the graph layer with both IDs predeclared
in a fake registry; give fixture/second a matching fake identity/lock before
checking edges. It is not a trusted-install fixture. The optional-unavailable
case tests an unavailable lock, not the default available registry. Source DTS
fixtures are v2 plugin-section fragments; insert them under the existing DTS
v2 container. No new outer DTS version directive is introduced by this packet.

## Stateful scenarios

`scenarios/` entries have closed top-level meaning: `format`, `property`,
`predicate`, `initial`, `steps`, `expected`. They describe deterministic fake-port
tests for the owning component, not a second product protocol. `initial` names
facts supplied by the fake registry/process/FS/audit/policy ports. Each `steps`
item names a model/host action and its operands; `expected` contains observable
counters/states/receipts. The test adapter must implement each listed action
explicitly and fail on an unknown field/action, never skip an unfamiliar case.

Common initial facts supply a verified fake artifact, ready session, Include
selection, scoped consent/organization/caller ceilings, epoch 1, available audit
and finite budgets. Synthetic file bytes are `abc`; a sealed upload is bound
to that file, epoch and invocation. `pre:1` is its initial resource identity,
`pre:2` a changed identity. Missing explicit numeric observations are not zero
assertions. Operations such as activate-candidate inject completed fake trust
verification independently of hash comparison; they are not cryptographic
signature fixtures. Timings are injected observations, never actual sleeps.

Scenario pairs test a positive control and a changed premise. Some changed
premises require successful degradation/recovery, not rejection: no library
host means successful plugin-free pack, a pre-publication crash retains old
activation, a post-commit hook failure preserves output, and a removed rule
requires repair despite a current marker. Their catalog `outcome` is decisive;
no `valid=false` is assigned to those scenarios. PP model mutation witnesses
must show a deliberately faulty transition violates the property; expected
outcome data alone is not model-checker evidence.

`git-cases.json` adds exact byte goldens for local protection. `utf8` strings
include their specified newline/CRLF bytes; hashes and lengths cover those
bytes. An input of null is absent or deliberately withheld because policy
forbids reading; inspect file_exists/selection rather than inventing bytes.
Coverage and repository facts are injected independently of source disclosure.
Stages/receipts and spy counters must match; no real Git operation is authorized
by running a fake-domain case. Real platform corpus remains PL-R/PL-A/PL-G work.

## Digest provenance and reproduction

`bytes/nist.json` copies the exact empty, `a`, and `abc` expected values from
`crates/dctx-hash/src/sha256.rs::tests::nist_short_message_vectors`. Decode the
base64 preimages; `abc.bin` means exactly `61 62 63`, **no newline**. The three-byte
artifact is intentionally not a native executable. The public key fixture is
RFC 8032 §7.1 test 1's public key, not a signing key or an approved publisher.

`digests.json` gives real lengths/hashes for the manifest's exact bytes, the
canonical compiled plan, action arguments and explicitly injected empty
selection/policy preimages. Remaining Git text/key fingerprints are derived
from their included preimages. Their expected values were authored using the
SHA-256 padding/compression arithmetic in dctx-hash's sha256.rs/compress.rs;
the Rust crate itself was not executed in this read/edit packet. The consuming
remote predicate must recompute **all** these identities using the public
`dctx_hash::Sha256` API and compare digest bytes plus byte_length, both one-shot
and incremental. Do not use a placeholder, a padded dummy hex string, the hash
of base64 text, or a core Git hash as an expected identity. The deliberately
wrong artifact case uses a real digest of *different included bytes*.

`schema`/`semantic` acceptance does not imply a signature, native format or OS
confinement check passed. There are no forged "valid signed release" fixtures.
PL-I/PL-REL own complete signed executable bundles and separate key manager grant
requirements. The permitted PL-C directory checks do not validate this corpus;
downstream remote conformance must do so before implementation promotion.

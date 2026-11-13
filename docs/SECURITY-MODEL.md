<!-- SPDX-License-Identifier: Apache-2.0 -->
# Security model

This document describes implementation boundaries and regression evidence for the
current engine and its CLI and stdio RPC. It is not a security
certification. See [SECURITY.md](../SECURITY.md) for reporting.

## Trust and assets

The caller chooses a place root and authorizes operations on it. Source bytes,
filenames, file sizes, directory contents, and protocol input can be hostile.
Other local users must not be able to substitute cache state. Assets include files
outside the chosen root, excluded content, output integrity, cache integrity, and
availability within the documented work bounds.

The executable, its dependencies, the account running it, its inherited
environment, and host-selected configuration/plugins are trusted. The process is
not a privilege boundary against another process with the same user identity,
an administrator, or a kernel/filesystem that lies about permissions or metadata.
Do not run it with elevated privileges on behalf of an untrusted client. A service
embedding the engine must authorize root selection and supply trusted ports and
policy; `Source` implementations supplied by a caller are responsible for their
own filesystem confinement.

Transport surfaces dispatch engine operations. They do not grant access to a
root the engine has not opened. RPC framing
does not constitute remote authentication or an Internet service.

## Paths, links, and races

`RelativePath` accepts at most 4,096 UTF-8 bytes. It refuses absolute paths, empty
interior components, dot/parent components, backslashes, colons (including drive
prefixes and alternate streams), control characters, and components ending with
a dot or space (Windows normalization aliases). Invalid UTF-8 directory names
are skipped; names rejected by the path type are skipped by the snapshot walk.
A snapshot may consequently be partial. This does not promise that arbitrary
source text is safe to display in a terminal: file contents are still untrusted.

The initial place path is canonicalized, so a symlink explicitly selected as the
root selects its target. On Unix the canonical path is opened one component at a
time with `openat`, `O_DIRECTORY`, and `O_NOFOLLOW`. The engine retains the root
handle. Reads, metadata, and listings resolve parents relative to directory
handles, and never follow a symlink in a source component. Content opens use
`O_NOFOLLOW | O_NONBLOCK`, then require a regular file using the opened handle's
metadata. A FIFO cannot block the read waiting for a writer. Reads use the opened
file's length, and stop after the requested cap plus one byte, even if a file grows.
A swapped ancestor cannot redirect an already opened directory to a link target.
A concurrent rename can leave an operation using the directory it already opened;
this is not an atomic snapshot of the namespace or of concurrently edited content.

Windows source access pins directory handles without write or delete sharing, checks
reparse-point attributes, and opens final content with
`FILE_FLAG_OPEN_REPARSE_POINT`. This is intended to refuse junctions and links
and prevent directory replacement while an operation holds the handles. Windows
regression tests are included, but were not executed by the Linux review run;
Unix runtime results must not be interpreted as Windows verification. Other
non-Unix/non-Windows targets have path checks, without the same handle guarantees.

Rule-file reads through `StdFs` (`dctx-adapter-fs`) also use no-follow component
resolution on Unix, regular-file checks, and bounded reads. Its metadata, directory
listings, and link reads resolve every directory from its parent handle with
`O_NOFOLLOW` too, so a directory swapped for a symlink fails the lookup instead of
reporting names, sizes, or link targets from outside the root. On Windows these
`StdFs` operations first pin each ancestor directory with list-directory access and
without write or delete sharing, and refuse reparse points, as the engine walker does;
this Windows code is compile-checked but was not executed by the Linux review run.
CLI nested rule listings use the engine source adapter. The CLI binary precheck uses an authorized engine source read after selection,
including ancestor selection, and refuses linked or excluded content. Rule loading
can fail closed on unsafe paths; global configuration
paths with symlinked ancestors may therefore be refused. Selection rules are
configuration, not executable shell text.

Evidence:

- `dctx-core::ids::tests::escaping_and_malformed_paths_are_refused` covers traversal,
  drive/stream syntax, control characters, Windows aliases, and overlong paths.
- `dctx` filesystem tests cover outside symlinks, swapping an ancestor after its
  handle is obtained, replacing the root pathname, final-link replacement, metadata
  and listings through links, sparse files, and FIFOs.
- `crates/dctx/tests/hostile_walk.rs` covers invalid UTF-8 names and source, control
  characters, and backslash names while retaining a readable positive control.
- `crates/dctx-cli/tests/security.rs` exercises CLI and RPC with outside
  file/directory symlinks and parent traversal, alongside an authorized read.
  Windows-only tests cover directory pinning and junction refusal.
- `dctx-adapter-fs` freshness tests inject a stat/read race and same-stamp byte edit;
  the former returns stale without content, and strict digest validation detects
  the latter. No filesystem-wide consistency guarantee follows from these tests.

## Resource bounds

Limits are checked by the engine as well as by framing adapters. These are work
and byte bounds, not a hard process RSS limit or a promise about CPU time for every
parser. Hosts can impose smaller operation budgets. They cannot raise a request
above the active engine ceiling.

| Input or operation | Enforced bound |
| --- | --- |
| One source file in the facade | 8 MiB maximum; smaller read caps are enforced on the handle and during the read |
| Snapshot directory depth | 256 |
| Snapshot entries | 100,000 by default; `DCTX_MAX_ENTRIES` permits 1 through 100,000,000 |
| Files, nodes, edges | Each limited by `DCTX_MAX_RECORDS`, default 1,000,000, maximum 100,000,000 |
| Snapshot source bytes | 1 GiB by default; `DCTX_MAX_SOURCE_BYTES` permits 1 byte through 16 GiB |
| One engine directory listing | Active entry ceiling, checked while collecting, before an unlimited vector can form |
| Template build/list input | 1 MiB per source or compiled plan; at most 100,000 entries in a template directory |
| Offline plugin bundle input | Host install policy ceiling before buffering (2 GiB by default); non-regular and linked paths refused |
| One rule file / `StdFs` source file | 1 MiB / 8 MiB, including reads that grow after metadata |
| CLI nested rule loading | 257 levels for rule-file lookup (recursion guard 256), 100,000 visited entries, 8 MiB of accumulated rewritten local rules |
| Query pages | Verb hard caps: at most 2,000 estimated tokens (1,000 for explain); smaller requested/host budgets intersect these |
| Read page | At most 400 source lines, also subject to encoded output budget |
| CLI RPC input | 1 MiB per line excluding LF; oversized frames are discarded and refused |

The environment ceiling overrides are trusted deployment settings. Raising them
raises potential work and memory use. Invalid numeric values are refused; the
strings are never passed to a shell. Query records, source totals, deadlines,
cancellation, output budgets, and cache admission have additional engine checks.
Directory collection may reach its safety ceiling before a smaller per-request
walk budget is applied, because the `Source` port returns a complete directory.

CLI template and offline-bundle loaders enforce their byte ceilings before
buffering. Sparse-input executable tests cover template build/list and offline
installation. The engine does not transparently decompress source archives. Legacy pack import
validates declared lengths, counts, offsets, total size, and a hard nesting limit
before accepting a document. This does not authorize unpacking arbitrary archives
or extracting paths onto disk.

Evidence includes the sparse-file and bounded-listing filesystem tests, operation
limit tests, `source_ceiling` tests (including shell-like invalid strings),
RPC oversized-frame tests, and `crates/dctx-pack/tests/legacy_import.rs` tests for malicious
lengths/counts/offsets and a 10,000-level depth bomb.

## Cache isolation

On Unix an existing cache root must be a real directory owned by the effective
user and not writable by group or others. Symlinked or writable ancestors are
refused, except root-owned sticky directories such as the system temporary
directory, or ancestors on a filesystem reported read-only by the kernel. Writable
ancestors must be owned by root or the current user. Read-only mount exceptions
assume that the host does not expose a writable alias to an untrusted writer. Newly created CLI cache directories use mode 0700 and cache files use
0600. Existing read/traverse permissions are not tightened: these checks defend
integrity and do not promise confidentiality for previously permissive caches.
Current permission checks cannot revoke handles obtained before a directory or
file was made private.
Object directories, object files, ledger files, and lock files are checked at
use for unsafe ownership/write permissions. Relative cache cleanup names use
the engine's portable component rules before the OS can normalize them. Lock opens refuse final symlinks.
Cache digests/checksums detect damage; they do not authenticate a malicious writer
who already has the running account's authority. Cache operations assume trusted
ancestors and ordinary local filesystem permission enforcement throughout their
lifetime; they are not a sandbox against a privileged or same-account mutator.
Windows cache privacy also depends on the ACLs supplied by the deployment; this
review did not verify cross-account Windows cache isolation.

Cache policy reads are bounded and refuse links and unsafe Unix ownership/write
permissions. Policy staging uses exclusive
creation, so an existing staging symlink cannot truncate another file. An existing
staging name causes a refusal. Tests cover outside sentinels, world-writable cache
roots/ancestors/object directories/object files/ledgers, and symlinked storage/locks.

The segment store library (`dctx-adapter-store` `StdFs::open`) applies the same root
rules on Unix: a symlinked or non-directory root is refused, a missing root is created
with mode 0700, an existing root must be owned by the effective user and not writable
by group or others, ancestors pass the checks above, and the root is tightened to 0700
through a no-follow handle. Store subdirectories are created 0700. Windows store
privacy depends on deployment ACLs.

Stdio RPC inherits the launching process's authorization and has no
separate authentication layer.

## Pack output markers

Packs are deterministic: the same files, rules, and template give the same bytes on
every run, so a pack can be a stable prompt-cache prefix. Boundary markers therefore
use sequential IDs (`F001`, `F002`, ...) and carry no per-pack random nonce.

Paths are encoded wherever the renderer writes them: tree entries, the `PATH`
attribute of file markers, and diff change summaries. `&`, `<`, `>`, and `"` become
`&amp;`, `&lt;`, `&gt;`, and `&quot;`; control characters and the Unicode line/paragraph
separators become `&#xHEX;`. A file name therefore cannot close an attribute, open or
close a marker, or start a new line. Paths without those characters are written
unchanged. The other marker attributes (`ID`, `LINES`, `SIZE`, `MOD`) are generated
numbers. Escaping targets the XML-style markers of the built-in templates; a custom
template that picks other marker characters gets the same encoding, not a guarantee
that its own markers cannot appear in a path.

File bodies are inserted verbatim. A file can contain text that looks exactly like a
closing marker followed by a forged opening marker with another `PATH` or a reused
ID. dctx does not detect or rewrite such text. Readers, including LLMs, should take
the file list and IDs from the directory tree, expect each ID's markers exactly once
in tree order, and treat file contents as untrusted data. Pack output is not a trust
boundary against prompt injection inside repository content.

Evidence: `crates/dctx-template/tests/path_escape.rs` renders a hostile file name
through the in-memory, streamed, plan, and diff renderers, checks that no forged
markers or raw separators appear, that ordinary paths keep their bytes, and that
output is identical across runs.

## Non-goals and operational limits

- Hard links, bind mounts, mounted filesystems, and files deliberately placed inside
  the root are not distinguishable from other authorized content by a path boundary.
- Selection rules apply to paths. They do not identify secrets in arbitrary
  content or track the provenance of bytes across renames.
- Concurrent writers can change source bytes or restore metadata. Freshness checks
  and digests do not create an atomic filesystem snapshot or prove author identity.
- There is no global request-rate limiter, tenant quota, hard RSS/CPU sandbox, or
  guarantee that an infinite stream of valid requests terminates. The stdio
  transports are intended for a trusted local account. Use OS process/resource
  isolation when serving untrusted callers.
- An attacker who controls the launch environment can select configuration/cache
  locations and raise documented ceilings. Hosts must sanitize inherited settings;
  accepting arbitrary environment overrides from a client is outside this model.
- Configuration and plugin grants must come from the authorized host. This review
  does not certify arbitrary plugin workers, dependencies, remote filesystems, or
  every supported OS/architecture. The plugin capability model has separate tests.

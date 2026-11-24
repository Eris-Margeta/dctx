# Offline verification of Linux and Windows CLI release archives

The release has two manifests named `SHA256SUMS`. The top-level one binds a **filename to compressed archive bytes**. The one inside an air-gap tarball also covers the **raw executable**. For Linux x64 and Windows x64, local fixtures passed both the selected-archive checksum and a byte comparison against the extracted executable. The offline install recipe leaves two different questions open: whether the operator received the intended architecture, and whether the signed SBOM describes those particular bytes.

**Evidence boundary.** Three checked-in tests passed with `python3 -B -m unittest discover -s tools/release -p 'test_supply_chain.py' -v`, the suite selected at `.github/workflows/ci.yml:142`. Two additional local archive probes and one same-tag selection simulation used disposable synthetic bytes. “Observed” below means a local result or a specific assertion in source. Workflow steps are release declarations. Cryptographic outcomes are inferred where stated. No release was fetched, signed, installed or executed. The local GitHub CLI lacked the attestation subcommand. Actual release contents, signer decisions, dependency completeness and native Windows execution remain unmeasured.

## Which files belong together

The operator chooses the expected tag independently. The guide uses `v1.3.2`, and scopes its recipe to CLI releases produced by this workflow (`docs/VERIFYING.md:5`, `docs/VERIFYING.md:21`). The asset names repeat across versions. On the producer side, preflight requires a release-shaped tag, the exact tag ref and equality of the tag without its leading `v` to `VERSION`. The build checks out that tag, builds the specified Cargo target, runs the built CLI with `--version`, and archives that file (`.github/workflows/release.yml:58`, `.github/workflows/release.yml:105`, `tools/release/build.sh:40`). Running the command checks its exit status. There is no comparison of its printed version in that step.

| Selected output | Linux x64 musl | Windows x64 MSVC |
| --- | --- | --- |
| Native archive | `dctx-x86_64-unknown-linux-musl.tar.gz` | `dctx-x86_64-pc-windows-msvc.zip` |
| Member to compare or install | `dctx` | `dctx.exe` |
| CycloneDX SBOM | `dctx-x86_64-unknown-linux-musl.cdx.json` | `dctx-x86_64-pc-windows-msvc.cdx.json` |
| Outer transfer archive | `dctx-x86_64-unknown-linux-musl-airgap.tar.gz` | `dctx-x86_64-pc-windows-msvc-airgap.tar.gz` |
| Matching npm package for this tag | `@tejl/dctx-linux-x64@1.3.2` | `@tejl/dctx-win32-x64@1.3.2` |

Each native archive, SBOM and outer transfer archive has a sidecar formed by appending `.sigstore.json` to its **complete** filename, including `.tar.gz` or `.zip`. Shared release files are `archive-provenance.jsonl`, `airgap-provenance.jsonl`, `SHA256SUMS` and each file's corresponding sidecar. For the five declared targets, the top-level manifest therefore has 17 payload rows (five native archives, five SBOMs, five air-gap tarballs and two provenance files). It has no row for a loose executable, itself or a signature sidecar. The Windows transfer archive is a tarball containing the original zip. The build matrix and signature loops define these names (`.github/workflows/release.yml:86`, `.github/workflows/release.yml:98`, `tools/release/supply-chain.py:40`, `tools/release/supply-chain.py:124`, `tools/release/supply-chain.py:131`, `tools/release/supply-chain.py:140`).

## Assertions from packaging to the installed copy

| Link | Exact assertion and evidence | What the consumer learns |
| --- | --- | --- |
| Native inventory | The writer inserts the supplied binary bytes, `LICENSE` and `NOTICE`. Across five targets the test requires identical archive bytes on the second packaging and the member list equal to the sorted three expected names. For tar it checks uid/gid 0, the fixture epoch, empty owner/group names, and modes 0755 for `dctx` and 0644 otherwise. For zip it checks a 1980-01-01 midnight timestamp and Unix creator value 3 for each entry (`tools/release/supply-chain.py:61`, `tools/release/test_supply_chain.py:38`, `tools/release/test_supply_chain.py:42`, `tools/release/test_supply_chain.py:48`). | Observed file layout. All targets use the same synthetic input in this test. A passing inventory cannot identify an ELF or PE machine type. Only the Linux x64 archive has a declared independent rebuild gate: two hashes equal, `cmp` succeeds, and the released archive matches the resulting SHA-256 before signing (`.github/workflows/reproducible.yml:103`, `.github/workflows/release.yml:178`). |
| Top-level checksum | The writer emits sorted rows of lowercase SHA-256, two spaces and the payload basename. It excludes signature sidecars and refuses an existing top-level manifest. The test changes the Linux archive and asserts its new digest differs from the stored row, then expects a reseal error (`tools/release/supply-chain.py:32`, `tools/release/supply-chain.py:141`, `tools/release/test_supply_chain.py:89`). The workflow requires all listed hashes to pass before upload and again before creating the release (`.github/workflows/release.yml:213`, `.github/workflows/release.yml:241`). | With the manifest authenticated, the guide selects rows whose second field equals the requested basename. If no row matches, awk exits 1 silently and the shell stops before the checksum command. For a matching row, GNU `sha256sum` reports `OK` (exit 0), `FAILED open or read` for a missing archive (exit 1), or `FAILED` for changed bytes (exit 1). The probe below checks both output streams (`docs/VERIFYING.md:20`, `docs/VERIFYING.md:40`, `docs/VERIFYING.md:41`). Without manifest authentication this measures consistency only. |
| Signature and trust | The signer requests a bundle, then verifies the same blob and bundle with the full release workflow identity at the expected tag and the GitHub Actions OIDC issuer, both commands requiring success. The sign test mocks two subprocess calls, checks the second begins with `cosign verify-blob`, contains the identity and issuer and has no `--insecure` option. An injected subprocess error propagates. The sealing test checks recorded signer calls and sidecar existence for every top-level payload (`tools/release/supply-chain.py:79`, `tools/release/test_supply_chain.py:71`, `tools/release/test_supply_chain.py:101`). | The native recipe authenticates **both** the archive and the manifest. The guide makes the expected certificate identity the full URL of the repository's release workflow at the chosen tag, with the exact issuer literal at `docs/VERIFYING.md:31`. A disconnected verifier additionally needs trusted cosign 3.1.4, an independently authenticated `trusted_root.json`, the expected tag/identity and the correct issuer. The bundle supplies the signature, certificate and transparency evidence. The guide initializes trust before transfer and passes `--trusted-root` offline (`docs/VERIFYING.md:28`, `docs/VERIFYING.md:44`, `docs/VERIFYING.md:142`, `docs/VERIFYING.md:173`). |
| Build provenance and SBOM | The workflow first attests native tarballs and zips, saves the resulting bundle as `archive-provenance.jsonl`, then separately attests the outer tarballs and saves `airgap-provenance.jsonl` (`.github/workflows/release.yml:186`, `.github/workflows/release.yml:196`, `.github/workflows/release.yml:203`, `.github/workflows/release.yml:211`). Cargo metadata from the CLI manifest, default features and matrix target feeds CycloneDX 1.5 JSON. The step copies the file under the target name and checks `git diff --exit-code -- Cargo.lock` (`.github/workflows/release.yml:135`). | The declared provenance subject is the digest of the selected **archive**. The guide requests the repository, signer workflow, source tag ref and denial of self-hosted runners. It mentions using a local attestation bundle (`docs/VERIFYING.md:63`, `docs/VERIFYING.md:70`). A cosign signature on the SBOM authenticates its JSON bytes. The workflow contains no assertion equating an SBOM subject digest to the native archive or executable. |
| Air-gap contents and checks | Staging copies the native archive, archive sidecar, SBOM, SBOM sidecar and native provenance. It reads the three named native members (regular files required on the tar branch), adds `VERIFYING.md`, hashes the seven non-sidecar files, signs the inner `SHA256SUMS`, packs everything, then signs the outer tarball (`tools/release/supply-chain.py:99`, `tools/release/supply-chain.py:108`, `tools/release/supply-chain.py:115`, `tools/release/supply-chain.py:135`). The test asserts the exact eleven-name set, equality of the loose executable bytes to the original fixture input, and SHA-256 equality for every row present in the inner manifest (`tools/release/test_supply_chain.py:79`, `tools/release/test_supply_chain.py:84`). | The disconnected recipe verifies the outer blob **before extraction**, authenticates the inner manifest, checks its rows, then installs the loose member. For Windows it calls for the same checks before copying `dctx.exe` (`docs/VERIFYING.md:173`, `docs/VERIFYING.md:179`, `docs/VERIFYING.md:190`). The outer signature covers the bundled sidecars too. The recipe does not separately invoke their verifiers or evaluate the native provenance statement. |
| Extracted and installed bytes | npm preparation requires an archive digest equal to the named checksum entry, reads the exact member and rejects an empty executable before writing packages (`tools/release/prepare-npm.py:30`, `tools/release/prepare-npm.py:63`). The packaging test corrupts the last archive (Windows), expects a checksum mismatch and asserts no output directory exists (`tools/release/prepare-npm.py:18`, `tools/release/test-packaging.py:93`). | For an installed npm copy, match the tag without `v` and the table's platform. For the example tag, the packed file is `tejl-dctx-linux-x64-1.3.2.tgz` or `tejl-dctx-win32-x64-1.3.2.tgz`. With files already available, compare the authenticated native member against both `package/bin/dctx` and the installed binary (Windows uses `dctx.exe` in each path). The Linux guide's two `cmp` calls enforce equality. Windows specifies checking all three SHA-256 values or using Git Bash. A printed hash or successful `--version` alone establishes no equality (`docs/VERIFYING.md:95`, `docs/VERIFYING.md:104`, `docs/VERIFYING.md:122`, `docs/VERIFYING.md:128`). In both local probes the raw and compressed digests differed, `cmp` returned 0 for a matching copy and 1 after appending one byte. |

## Observed selector and checksum failures

The documented selector redirects matching rows to `selected.sha256`. A request containing a newline, ESC and a double quote matched no row. The selected file was empty, stdout and stderr were empty, and the recipe exited 1. The operator receives no asset name or explanation for this failure. When the row exists, GNU `sha256sum` names the archive on stdout. For a missing file, stderr gives the file-open error and an unreadable-file warning. For changed bytes, stderr warns of a checksum mismatch. The probe runs the guide's two commands under its `set -eu` and checks their actual output in the C locale.

```sh
python3 -B - <<'PYTEST'
import os
import subprocess
from hashlib import sha256
from pathlib import Path
from tempfile import TemporaryDirectory

guide = Path("docs/VERIFYING.md").read_text().splitlines()
start = next(i for i, line in enumerate(guide) if line.startswith('awk -v name="$ASSET"'))
assert guide[start + 1] == "sha256sum --check selected.sha256"
recipe = "\n".join([guide[guide.index("set -eu")], *guide[start:start + 2]])

with TemporaryDirectory(dir=".") as tmp:
    root = Path(tmp)
    names = ("dctx-x86_64-unknown-linux-musl.tar.gz", "dctx-x86_64-pc-windows-msvc.zip")
    payload = b"fixture"
    rows = {name: f"{sha256(payload).hexdigest()}  {name}\n" for name in names}
    (root / "SHA256SUMS").write_text("".join(rows.values()))

    def verify(asset):
        result = subprocess.run(["sh", "-c", recipe], cwd=root, capture_output=True,
                                text=True, env={**os.environ, "LC_ALL": "C", "ASSET": asset})
        return result.returncode, result.stdout, result.stderr

    assert verify('absent\n\x1b"') == (1, "", "")
    assert (root / "selected.sha256").read_bytes() == b""
    for name in names:
        assert verify(name) == (
            1, f"{name}: FAILED open or read\n",
            f"sha256sum: {name}: No such file or directory\n"
            "sha256sum: WARNING: 1 listed file could not be read\n",
        )
        assert (root / "selected.sha256").read_text() == rows[name]
        (root / name).write_bytes(payload)
        assert verify(name) == (0, f"{name}: OK\n", "")
        (root / name).write_bytes(payload + b"!")
        assert verify(name) == (
            1, f"{name}: FAILED\n", "sha256sum: WARNING: 1 computed checksum did NOT match\n",
        )
PYTEST
```

## Findings

1. **The offline install can accept a different same-tag architecture. High confidence in the missing selection check.** The transfer list carries the outer blob and sidecar but omits the top-level signed filename-to-digest mapping (`docs/VERIFYING.md:154`). The install verifies the chosen blob under a tag-wide identity, then checks whichever inner manifest it contains (`docs/VERIFYING.md:169`, `docs/VERIFYING.md:182`). In a bounded probe, an arm64 air-gap tarball under the x64 filename had a different outer digest from the expected x64 file, yet every inner row passed and the extracted bytes were the arm64 fixture. **Inference:** moving a genuine same-tag blob with its genuine sidecar preserves its blob signature. An operator could install the wrong Linux binary and discover the error only on execution. This probe measured the name and checksum behavior with placeholder sidecars.

2. **The signed SBOM has no checked link to the selected binary. High confidence.** Generation selects a target and assigns a filename. Sealing signs that filename's contents, and the fixture succeeds with only `{"bomFormat":"CycloneDX"}` (`.github/workflows/release.yml:137`, `tools/release/supply-chain.py:132`, `tools/release/test_supply_chain.py:59`). **Inference:** an incorrect-target SBOM assigned the expected name before sealing could pass the blob signature and manifest checks. An offline reviewer can identify the exact SBOM the release endorsed, but cannot use these checks to establish which executable its inventory describes. No real SBOM content or dependency graph was measured.

3. **Disconnected signature and provenance failures lack consumer test evidence. High confidence in the coverage gap.** The seal fixture writes placeholder sidecars and provenance. The signer error test makes the first subprocess call raise, so it never exercises a failed consumer verification (`tools/release/test_supply_chain.py:60`, `tools/release/test_supply_chain.py:110`). **Expected user behavior:** missing blob, sidecar or root, wrong trust root, issuer, identity/tag or changed signed bytes should produce a nonzero verifier exit. With `set -eu` this stops the documented install before extraction for outer failures, or before copying for inner failures (`docs/VERIFYING.md:167`, `docs/VERIFYING.md:179`). A correct local provenance check should also exit nonzero for a missing bundle or a digest, source ref, repository or signer that fails the requested policy. The provenance example uses the online form. The offline section neither runs a local provenance check nor lists the outer provenance bundle for transfer (`docs/VERIFYING.md:63`, `docs/VERIFYING.md:154`). A successful install therefore supplies no independent result for the expected attestation subject or source policy. Actual verifier messages and disconnected cryptographic outcomes remain unmeasured.

4. **A manifest can omit a required row without failing the current checksum assertions. High confidence.** The writer currently includes the loose executable and native archive. The test checks the tar member set, then iterates the rows the manifest happens to contain (`tools/release/supply-chain.py:117`, `tools/release/test_supply_chain.py:83`, `tools/release/test_supply_chain.py:85`). The consumer likewise checks the listed rows (`docs/VERIFYING.md:182`). **Inference:** a future producer omission of just the executable row would leave the other hashes passing. The outer signature would still protect the tarball, but the operator would lose the explicit per-executable digest check. This differs from a **listed** missing or damaged file, which the checksum command rejects.

## Follow-up cards

| Proposed title | Write seams | Executable acceptance check |
| --- | --- | --- |
| Bind the offline install to the requested platform (finding 1) | `docs/VERIFYING.md`, `tools/release/test_supply_chain.py` | `python3 -B tools/release/test_supply_chain.py ReleasePayloads.test_offline_target_selection` (new test). Transfer and authenticate the top-level manifest and check the exact outer basename and digest before extraction. Both x64 formats pass with matching material. Renaming an arm64 pair to x64 fails before any install destination is created. |
| Bind the CycloneDX inventory to its native subject (finding 2) | `.github/workflows/release.yml`, `tools/release/supply-chain.py`, `tools/release/test_supply_chain.py`, `docs/VERIFYING.md` | `python3 -B tools/release/test_supply_chain.py ReleasePayloads.test_sbom_subject` (new test). For both native formats require the documented target, release version and SHA-256 subject of the chosen archive or member. A missing digest, changed subject or swapped target fails even if the JSON's own checksum is correct. |
| Exercise disconnected signature and provenance policy (finding 3) | `docs/VERIFYING.md`, `tools/release/test_offline_verify.py`, bounded static test material under `tools/release/fixtures/offline/` | `python3 -B tools/release/test_offline_verify.py` (new suite). With verifier tools pre-provisioned, networking disabled and independently pinned fixture roots, verify both formats and both provenance scopes. Remove the root or sidecar, change the expected identity, issuer or tag, or substitute the other subject's attestation. Each negative must exit nonzero before install. No runtime signing or fetching. |
| Require complete manifests and compare the extracted copies (finding 4) | `tools/release/test_supply_chain.py`, `docs/VERIFYING.md` | `python3 -B tools/release/test_supply_chain.py ReleasePayloads.test_manifest_inventory` (new test). Assert exact row-name sets and unique entries at both levels, recompute every digest, then compare the loose executable with the member of the original tar or zip. Remove only the raw-binary row or alter either copy and require rejection. |

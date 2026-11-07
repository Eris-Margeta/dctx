<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# Security Policy

## Supported versions

The current major release receives bug fixes and security fixes in its latest
release. When a new major is released, the previous major receives security fixes
for **six months after the new major's release date**, then reaches end of support.
Older majors receive no fixes. Upgrade to the latest patch within a supported
major before checking whether an issue is resolved. We publish fixes in new patch
releases; we do not promise a separate patch for every historical minor version.
This public policy has no LTS commitment beyond that six-month transition. Include the exact
version or commit and build features so maintainers can assess affected versions.

## Reporting a vulnerability

**Do not disclose vulnerabilities in public issues or pull requests.** Use
[GitHub private vulnerability reporting](https://github.com/Eris-Margeta/dctx/security/advisories/new)
or email [info@tejl.com](mailto:info@tejl.com) with a subject beginning
`[dctx security]`. If GitHub's private reporting form is unavailable, use email.
Maintainers must enable private vulnerability reporting in repository security
settings; that GitHub setting is separate from this policy file.

Include, where possible:

- Affected version or commit, OS/architecture and build features.
- The affected component and potential impact (for example ignored-file content
  disclosure, untrusted plugin execution, or signature verification bypass).
- Exact reproduction steps and a minimal synthetic fixture or proof of concept.
- Expected and actual results, with sanitized diagnostics.
- Whether the issue has been shared elsewhere, and a preferred contact method.
- A name or alias for credit, or a request to remain anonymous.

Never send private keys, tokens, production data or a generated context export
containing confidential code. Use synthetic examples and redact sensitive values.
Test only systems and data you own or have explicit permission to assess.

## What counts as a vulnerability

dctx is an offline tool, but untrusted repositories, files, archives and plugin
inputs still cross security boundaries. Report issues such as:

- Disclosure of ignored, excluded or secret-file contents, including through
  generated context, caches, diagnostics or plugin access.
- Arbitrary code execution, sandbox or capability bypass, unsafe plugin execution,
  or command injection caused by processing untrusted input.
- Path traversal, symlink escapes, unintended file modification or unsafe archive
  extraction outside an authorized destination.
- Signature, checksum, provenance or update verification bypass; compromised
  release artifacts or dependencies affecting supported builds.
- A reproducible resource-exhaustion attack from a small malicious input that
  defeats documented bounds and makes the tool unusable.

Ordinary output mistakes, unsupported language coverage, feature requests and
performance issues without a security boundary impact belong in public issues.
Use private reporting when unsure. Offline operation does not make confidential
local data or the release supply chain out of scope.

## Handling and disclosure

We acknowledge private vulnerability reports **within 7 days**. We investigate
impact, identify affected supported versions and coordinate a fix or mitigation
and public disclosure with the reporter. Remediation timing depends on severity
and available maintainers; the acknowledgment window is not a promised fix deadline.
If acknowledgment has not arrived within 7 days, follow up through the other
private channel. Credit is subject to the reporter's consent.

We will not take legal action against individuals who discover and report
security vulnerabilities in good faith and in accordance with this policy.

Please coordinate public disclosure so users can receive a fix or mitigation.
For architectural boundaries and current implementation limits, see the
[README](https://www.dctx.internet.technology/docs/readme/#security--privacy) and [plugin guide](https://www.dctx.internet.technology/docs/plugins/).
Those descriptions are not a security certification.

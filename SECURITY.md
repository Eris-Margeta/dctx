# Security Policy

## Supported development line

Security fixes target the latest 1.x release and the current `main` development
line. The current release is **1.2.0**. Upgrade to the latest release before
checking whether an issue is resolved. Older releases have no published backport
or long-term support commitment; separate [maintenance agreements](MAINTENANCE.md)
may define additional coverage. Include the exact version or commit and build
features so maintainers can assess whether current code is affected.

## Reporting a vulnerability

**Do not disclose vulnerabilities in public issues or pull requests.** Email
[eris.margeta@gmail.com](mailto:eris.margeta@gmail.com) with a subject beginning
`[dctx security]`. This private reporting route does not depend on GitHub private
vulnerability reporting being enabled.

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

## Handling and disclosure

Maintainers will review reports, investigate impact and coordinate fixes and
public disclosure with the reporter. Response and remediation times depend on
availability and severity; this policy does not promise a fixed service level.
If you have not received a reply after a week, send a follow-up to the same address.
Credit is subject to the reporter's consent.

We will not take legal action against individuals who discover and report
security vulnerabilities in good faith and in accordance with this policy.

Please coordinate public disclosure so users can receive a fix or mitigation.
For architectural boundaries and current implementation limits, see the
[README](README.md#security--privacy) and [plugin guide](docs/guides/PLUGINS.md).
Those descriptions are not a security certification.

# Security Policy

## Supported development line

dctx is pre-1.0 software in active development. Security fixes target the current
`main` development line. There is no published long-term support or backport
commitment for older releases. There is no maintained 1.x release line. Include the exact commit and build features in a report so that
maintainers can assess whether the current code is affected.

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
[README](https://www.dctx.internet.technology/docs/readme/#security--privacy) and [plugin guide](https://www.dctx.internet.technology/docs/plugins/).
Those descriptions are not a security certification.

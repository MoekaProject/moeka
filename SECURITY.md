# Security policy

## Report a vulnerability privately

Do not disclose suspected vulnerabilities, exploit instructions, credentials, or private documents in public issues, pull requests, or discussions.

If GitHub private vulnerability reporting is enabled for the source repository, use **Security → Report a vulnerability** in [MoekaProject/moeka](https://github.com/MoekaProject/moeka/security). The [private report form](https://github.com/MoekaProject/moeka/security/advisories/new) is available only when that feature is enabled.

If private reporting is unavailable, request a private security contact through [MoekaProject/community](https://github.com/MoekaProject/community/issues). Say only that you need a private channel for a security report. **Do not include vulnerability details, affected components, reproductions, or sensitive attachments in that public request.** Wait for a private channel before sharing them.

## What to include in the private report

- The affected Moeka version or source commit, operating system, architecture, and distribution channel.
- A description of the issue, its impact, and the conditions required to trigger it.
- Minimal reproduction steps or a harmless proof of concept, using synthetic data where possible.
- Any mitigation or proposed fix you have identified.

Keep testing limited to systems and data you are authorized to use. Redact unrelated personal information and secrets.

## Versions and follow-up

There is no published long-term security-support matrix or guaranteed response timeline. Include the exact affected version; do not assume older releases receive backported fixes. Published downloads and release notes remain in [MoekaProject/community releases](https://github.com/MoekaProject/community/releases).

Coordinate public disclosure through the private reporting channel so a fix or mitigation can be discussed before technical details are published.

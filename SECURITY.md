# Security policy

Report vulnerabilities privately through this repository's **Security** tab → **Report a vulnerability**. Use [GitHub private vulnerability reporting](https://github.com/daplabs/maxed/security/advisories/new); do not open a public issue with exploit details or sensitive data. Maintainers must enable this route before public release.

Include the affected version or commit, impact, reproduction steps and a minimal synthetic example where possible. Never attach raw game captures, access tokens, account data, signing keys or other players' information. Describe sensitive evidence first so a safe reproduction can be arranged.

Reports in scope include updater signature and redirect validation, installer or release integrity, passive-capture and scrubbing boundaries, accidental network disclosure, and unauthorized access to or deletion of local data. Recording ownership protects a cooperative local namespace; it is not a security boundary against another process running as the same Windows user.

Maxed is pre-release software. Fixes are developed on `main`; no older release has a promised security-support window. Maintainers will assess reports privately and coordinate a fix and disclosure with the reporter. No response deadline is promised.

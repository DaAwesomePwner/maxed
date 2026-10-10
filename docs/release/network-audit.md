# Network audit

Maxed's own application code only initiates the manual GitHub update flow. Npcap observes game traffic passively; recording and summaries remain local. The installer can bootstrap WebView2 when missing. The embedded WebView2 runtime also makes independent Microsoft requests, disclosed in [PRIVACY.md](../../PRIVACY.md#outbound-requests). This policy is not a zero-outbound promise.

## Runtime allowance

Only WebView2 processes may use the following exact hosts; root/unknown process roles do not inherit the allowance.

| Hosts | Ports |
| --- | --- |
| `substrate.office.com`, `config.edge.skype.com`, `edge.microsoft.com` | 443 |
| `msedge.b.tlu.dl.delivery.mp.microsoft.com`, `msedge.f.tlu.dl.delivery.mp.microsoft.com`, `msedge.b.dl.delivery.mp.microsoft.com`, `msedge.f.dl.delivery.mp.microsoft.com` | 80 or 443 |

The port-80 exception is for signed component payloads; signature verification does not encrypt transport. Runtime configuration/component updates retain their defaults. No suffix wildcards or general Microsoft-host exception is allowed.

Historical investigation reproduced the account-related destination in a separate minimal WebView2 host, while configuration/component requests were visible in Chromium diagnostics. The account request's exact contents and initiating feature were not established. No upload of local Maxed game data was established. These observations support disclosure of runtime behavior, not universal endpoint coverage or current installed-release acceptance.

## Why WebView2 network defaults are kept

Targeted feature-disable experiments do not remove the account-related
connection. Disabling component updates suppresses security maintenance,
including CRLSet certificate-revocation data and PKI metadata; disabling Edge
configuration also removes emergency feature rollback controls. The production
policy therefore keeps runtime network defaults and discloses their traffic.
These browser security services are separate from the Rust updater's TLS roots.

## Update and installer allowances

The `update` policy allows the Maxed root process to contact `github.com` and
`release-assets.githubusercontent.com` on port 443. The `installer` policy allows
eligible installer/bootstrap process roles to contact these exact hosts:

| Hosts | Port |
| --- | --- |
| `go.microsoft.com`, `msedge.api.cdp.microsoft.com` | 443 |
| `msedge.sf.dl.delivery.mp.microsoft.com`, `msedge.sf.tlu.dl.delivery.mp.microsoft.com`, `msedge.sb.dl.delivery.mp.microsoft.com`, `msedge.sb.tlu.dl.delivery.mp.microsoft.com` | 443 |
| `msedge.f.dl.delivery.mp.microsoft.com`, `msedge.f.tlu.dl.delivery.mp.microsoft.com`, `msedge.b.dl.delivery.mp.microsoft.com`, `msedge.b.tlu.dl.delivery.mp.microsoft.com` | 80 |

WebView2 descendants retain the separate runtime allowance above. Neither policy
allows arbitrary Microsoft hosts or relaxes process-role checks.

## Collector and procedure

`tests/packaging/network-audit.ps1` is a metadata-only process-tree sampler. It reads socket/process identity and the local DNS cache, not packet payloads, command lines, settings or recordings, and performs no new DNS lookup.

```powershell
pwsh -NoProfile -File tests/packaging/network-audit.ps1 -SelfTest
pwsh -NoProfile -File tests/packaging/network-audit.ps1 -RootProcessId $maxedPid -OutputDir $baselineEvidence -Seconds 60 -Phase app-baseline -Policy app
pwsh -NoProfile -File tests/packaging/network-audit.ps1 -RootProcessId $maxedPid -OutputDir $updateEvidence -Seconds 60 -Phase manual-update -Policy update
pwsh -NoProfile -File tests/packaging/network-audit.ps1 -RootProcessId $installerPid -OutputDir $presentEvidence -Seconds 60 -Phase webview-present -Policy app
pwsh -NoProfile -File tests/packaging/network-audit.ps1 -RootProcessId $installerPid -OutputDir $installerEvidence -Seconds 60 -Phase webview-absent -Policy installer
```

Supply live process IDs and fresh local evidence directories. Exercise installed startup, capture, demo, settings, manual update, shutdown and installer cases both with and without WebView2. Review `network-report.json` and `diagnostics.txt`; preserve sanitized artifact/OS/runtime versions, phase durations, process identities, allowance counts, violations and blind spots.

## Verdict limits

DNS cache correlation is not process-specific DNS or TLS-host proof. Every correlated name must be approved for that port. Mixed approved/unapproved or wrong-port names are inconclusive; retain every name instead of choosing the favorable one. Unresolved runtime port-80/443 destinations are inconclusive; endpoints with no approved names, unsupported ports or private/link-local/ULA destinations fail.

Loopback TCP requires a mirrored peer in the owned process tree. Missing/foreign peers, manual proxies/PAC, unreadable proxy state, missing counters, lost roots, unobservable UDP peers and unverified conditional native-delivery candidates yield `not_measured`. Auto-detect alone is recorded without performing WPAD discovery. An observed violation takes precedence over later incomplete samples.

A gap over 3,000 ms from collection start to first sample, between sample starts, or from last sample to end yields sticky `sampling_coverage_insufficient`. Fewer than `ceil(requested_seconds * 1000 / 3000)` complete samples also prevents acceptance. Exit 0 means a complete passing observation window; exit 1 includes both `fail` and `not_measured`.

A sampling pass proves only what was observed in that window. Supplement it with destination-only, process-attributed trace evidence for startup, shutdown, short-lived installer requests and UDP/QUIC. Independently verify HTTPS and exact repository/release paths and redirects through native transport tests and sanitized delivery evidence; sockets cannot establish HTTP paths. Never retain payloads, signed URL queries, command lines or user paths in public reports. Installed/all-action privacy acceptance remains a separate [release gate](CHECKLIST.md).

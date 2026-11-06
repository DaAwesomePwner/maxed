# Updater signing

Maxed's updater public key is pinned in `app/src-tauri/tauri.conf.json`. Updater signatures authenticate installer bytes and version; they do not provide Windows Authenticode signing or SmartScreen reputation.

## Key custody

Assign a maintainer responsible for the signing key, restrict access and keep an encrypted offline backup separately from the primary secret store. Document recovery access privately. CI secrets cannot be read back and are not a backup. Never commit keys/passwords or put them in arguments, transcripts, shell history, logs, `.env` files or reports.

Only the package process receives `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Clear both variables afterward and dispose the dedicated process. Ordinary executable builds need no signing key. An intentionally unencrypted key can have an empty password; a missing key must fail packaging. Disposable test keys never become production configuration.

```powershell
try {
  powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build-release.ps1 -Package
  if ($LASTEXITCODE) { throw 'Package build failed.' }
} finally {
  $env:TAURI_SIGNING_PRIVATE_KEY = $null
  $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = $null
}
```

Populate the process environment securely before this wrapper; no secret retrieval command is prescribed. The build verifies the signature against the configured public key and checks the actual extracted payload. Follow [release staging](UPDATES.md) and the [checklist](CHECKLIST.md).

## Release controls

Restrict `release-signing` and `release-publish` to `v*` tag deployments. Signing currently has no required reviewer; `release-publish` requires maintainer approval before publication and channel promotion. Only the package step receives signing secrets. Only the publish job has `contents: write`, with `GH_TOKEN` set to the implicit `github.token` in its publish step; the workflow default remains `contents: read`. Pull-request tests and ordinary builds must not receive signing secrets.

Restrict who can create release tags and block updates/deletions of existing `v*` tags; a workflow event filter is not a trust boundary. Verify remote protections before publication rather than treating a historical configuration as current evidence. Failed promotion after immutable publication requires the [manual recovery procedure](UPDATES.md#serialized-promotion), not retagging.

## Rotation and recovery

Installed clients trust one pinned public key. Planned rotation needs two releases:

1. Generate a new key pair locally with the Tauri signer and secure its primary and offline copies.
2. Prepare release N with the new public key embedded, signed by the old private key so existing clients can accept it.
3. After migration, replace the `release-signing` environment secrets with the new private key and password, then sign N+1 with the new key. Keep the old key only while required for supported clients, then destroy retained copies under the custody procedure.

**Tooling prerequisite:** the current build and metadata scripts verify against the public key in the same checkout. They cannot silently accept the old signing key with a newly pinned key. A transition needs a separately reviewed build/verification mechanism and disposable-key end-to-end tests before executing this plan; do not disable signature checks to force rotation through.

If the old key is lost, there is no in-band recovery for clients that pin it. Publish a reviewed build pinning the replacement key and require manual installation from the release page. For suspected compromise, stop promoting new channel metadata, communicate the risk and arrange migration. An attacker with the old private key can still sign artifacts for clients that trust it; keeping a pointer on a known-good release does not revoke that trust.

Announce any forced rotation in the release notes, including the manual-install requirement for clients that trust the replaced key.

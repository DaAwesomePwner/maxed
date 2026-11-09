# Local UI review on Windows

`scripts/windows/` builds the revision under review and drives the real desktop
app through WebView2's DevTools protocol (CDP) on `127.0.0.1`. Use it to look at
and script UI changes on a Windows desktop. It complements the `windows-2022`
**gui-smoke** CI check; it does not replace it. A local review does not establish
live capture, game-client, installer or hosted-runner acceptance.

Requirements: the [Windows setup](../../CONTRIBUTING.md#windows-setup)
prerequisites (Node.js 22.16.0, PowerShell 7) and an interactive desktop session.
The driver needs no extra dependencies.

## Build

```powershell
pwsh -NoProfile -File scripts/windows/build-review.ps1
$exe = (Get-Content target/ui-review/build.json -Raw | ConvertFrom-Json).exe
```

The builder runs `npm ci`, builds the UI, then builds a native debug executable
with `custom-protocol` and prints its path and build time. It reuses
`Build-SmokeApp` from the [GUI smoke harness](../../tests/gui-smoke/README.md),
so the profile directories are compiled in under `target/ui-review/profile`.
Tauri ignores `APPDATA` / `LOCALAPPDATA` overrides, so this is the only way to
isolate them. Your normal Maxed data is never read or shown, and the profile
survives relaunches. Stop the app before rebuilding.

`-RealProfile` builds a separate executable under `target/ui-review/build-real`
that uses your normal profile, and prints a warning. Avoid it for routine review.
An installed executable also uses your normal profile.

## Drive

```powershell
node scripts/windows/ui-drive.mjs launch $exe
try {
    node scripts/windows/ui-drive.mjs shot target/ui-review/app.png
    node scripts/windows/ui-drive.mjs eval 'document.title'
    node scripts/windows/ui-drive.mjs click 'Runs'
    node scripts/windows/ui-drive.mjs console --ms 3000
} finally {
    node scripts/windows/ui-drive.mjs stop
}
node scripts/windows/ui-drive.mjs status
```

`--help` lists the commands. `launch <exe> [--port N]` (default port 9238) sets
the WebView2 debugging and background-rendering arguments and waits up to 30
seconds for the main page. It records the PID, executable, port and start time
in `target/ui-review/session.json`. It refuses an occupied port or a live
matching session. Later commands use that record.

- `eval` awaits promises.
- `click` takes a CSS selector or exact visible navigation text, and needs a
  single enabled match.
- `console` reports the errors it collects during its observation window. It
  does not prove that startup was clean.

`stop` affects only the recorded process tree:

1. It verifies the executable and start time.
2. It asks the window to close, then force-stops the app after two seconds.
3. It terminates descendants only after rechecking their creation times. It
   fails if any survive.

A reused or inaccessible PID counts as stale. `status` reports it, and `stop`
clears the record without killing anything. Remove a leftover `session.lock`
only after the interrupted driver command has exited.

Playwright MCP can attach to the same app with
`--cdp-endpoint http://127.0.0.1:9238`.

Run the driver's unit tests with `node --test scripts/windows/*.test.mjs`.

## Screenshots and privacy

Prefer `shot`, which captures only the app. Use
`desktop-shot <out.png>` only for native UI such as dialogs or the tray. It
captures every monitor, including unrelated private windows. Output paths must
be new files in existing directories under the worktree's ignored `target/`.
`desktop-shot` also rejects paths under reparse points, such as some OneDrive
folders.

Never commit screenshots. Attach only sanitized images to pull requests or
issues: no player names, account data or unrelated windows. See the
[privacy rules](../../AGENTS.md#privacy-and-evidence).

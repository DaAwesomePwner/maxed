# Windows desktop GUI smoke

This harness builds and launches the real debug Tauri app with its bundled Svelte
UI, in a fresh temporary profile. It checks the normal, unconfigured Setup screen
through WebView2 CDP, observes it for ten seconds, writes a PNG and JSON reports,
and terminates the app and its descendants. It does not install Npcap, capture
traffic, discover game assets, or launch a game.

## CI invocation

Run on GitHub-hosted `windows-2022`. Prerequisites: PowerShell 7,
Node **22.16.0**, the pinned Rust toolchain, VS 2022 Build Tools, Windows SDK and
WebView2 Evergreen. The workflow restores Node, Rust and locked dependencies;
the Windows image must supply the native build tools and WebView2 runtime.
The harness launches a desktop process, not a headless browser. Desktop/session
availability and WebView2 rendering on the hosted image need a real CI run;
macOS cross-compilation cannot verify them. Missing runtime or unavailable CDP
fails the smoke rather than skipping it. Do not run the app on macOS.

`.github/workflows/gui-smoke.yml` runs daily at 09:47 UTC (`47 9 * * *`) on the
default branch, `main`, for pull requests targeting `main` (including forks), and
manual dispatch. Pushes to `main` do not trigger it. PR runs
cancel superseded runs; the single GUI job has a 45-minute timeout and read-only
contents permission, with no secrets or environment scopes. It is separate from
the reusable `tests.yml`, so release packaging never requires the GUI runner.

The job uses the checkout's default Cargo target and disposable hosted-runner
storage; it has no persistent cache paths or cache actions. After restoring the
pinned tools, it runs both harness unit suites before invoking the launcher.

From the repository root, using a **fresh output directory**:

```powershell
pwsh -NoProfile -File tests/gui-smoke/Invoke-GuiSmoke.ps1 `
    -Port 9238 -TimeoutSeconds 60 `
    -OutputDir "$env:RUNNER_TEMP\gui-smoke-artifacts"
exit $LASTEXITCODE
```

The workflow uses `$RUNNER_TEMP/gui-smoke-<run_id>-<run_attempt>/` as its fresh
artifact directory. It uploads only that directory with `always()`, a three-day
retention period, and hidden files excluded. `if-no-files-found: warn` is deliberate:
checkout/tool restoration or tests can fail before there are smoke artifacts, and
upload should preserve any available diagnostics without adding a second failure
for missing output. A successful launcher still requires its PNG and reports.
The job timeout also bounds dependency installation/build. The launcher runs
`npm --prefix app/ui ci`, the UI build, and locked Cargo metadata/build itself.
Do not reuse this diagnostic executable for ordinary development or packaging.

- `screenshot.png`: `Page.captureScreenshot` output, also attempted after an
  attached-session failure. It may be absent if discovery/attachment fails.
- `report.json`: CDP result, reason, bounded error list, observation duration and
  screenshot status, plus bounded page/event `diagnostics` on attached-session
  failures. It is absent if the launcher fails before Node starts.
- `launcher.json`: overall exit code, launcher failure, cleanup failures and
  `profile_lockers` diagnostics from any failed deletion attempts.
- `owner.json`: the launcher PID, start time, executable and output-directory
  identity used by the final cleanup step; no process-name-wide kill is used.

Node resolution uses the first application on PATH (the toolcache entry prepended
by `actions/setup-node`), even if Node is also installed machine-wide. The launcher
and native launcher test share a resolver that requires one rooted path to an
existing file and fails clearly when Node is missing. Native start failures report
the application path, working directory and original Win32 error without dumping
the child environment.

The launcher propagates Node's exit code; launcher/cleanup failures return 1.
Existing artifact filenames are refused to avoid uploading a stale screenshot.
The CDP script can also be called against an **already isolated smoke-mode app**:

```powershell
node tests/gui-smoke/cdp-smoke.mjs --port 9238 --timeout-ms 60000 --output-dir <fresh-directory>
```

The CLI never shortens the ten-second observation. Discovery, WebSocket attach,
CDP calls, mount wait and observation share a deadline. Failure page diagnostics
have a separate one-second allowance (with a 750 ms script execution timeout),
followed by up to three seconds for screenshot capture. The launcher bounds Node
by the requested timeout plus ten seconds. It checks for a listening port before building and
again immediately before launch; it never connects to an intentionally reused port.

## Build and isolation decisions (pitfalls 1–3)

**Bundled UI:** `app/src-tauri/tauri.conf.json:6–10` specifies both the dev server
and `../ui/dist`; `app/src-tauri/Cargo.toml:43` enables `tauri/custom-protocol`.
The launcher builds the UI first, then runs:

```text
cargo build --locked -p maxed-app --bin maxed --features custom-protocol --target x86_64-pc-windows-msvc
```

There is no `--release`, dev server, or bundler. Explicitly building the UI is
necessary because plain Cargo does not execute `beforeBuildCommand`. The launcher
uses `cargo metadata --locked --no-deps --format-version 1`'s `target_directory`
and appends `x86_64-pc-windows-msvc/debug/maxed.exe`, honoring `CARGO_TARGET_DIR`.
It sets `CARGO_PROFILE_DEV_DEBUG_ASSERTIONS=true` on Cargo only and refuses
inherited compiler flags that override debug assertions.

**Browser arguments:** `app/src-tauri/src/lib.rs:1081` sets the builder's browser
arguments using `window_policy.rs:7`. Wry 0.57.0 passes them as environment options
to WebView2 (`src/webview2/mod.rs:300,333,349`). Microsoft's
[WebView2 environment API](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/webview2-idl#createcorewebview2environmentwithoptions)
documents that `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` is **appended** to these
options; the published
[Win32 API source](https://github.com/MicrosoftDocs/webview2-win32-reference/blob/main/1-0-3537-50/webview2-idl.md#createcorewebview2environmentwithoptions)
also states this explicitly. The environment value is child-only and contains:

```text
--remote-debugging-port=<port> --disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,CalculateNativeWinOcclusion --disable-backgrounding-occluded-windows --disable-renderer-backgrounding --disable-background-timer-throttling
```

Chromium uses the last instance of a switch. WebView2 documents feature merging
with built-in features, but the harness explicitly preserves the app's list
rather than depending on duplicate-switch merging. `Get-SmokeBrowserArguments`
reads the plain `DEFAULT_BROWSER_ARGS` string from `window_policy.rs`, unions its
disabled features with `CalculateNativeWinOcclusion`, removes duplicates and
emits one `--disable-features` switch. If the constant changes to an unrecognized
form, launch fails with a request to review the merge. Other app arguments,
including the hardware-acceleration policy's GPU flags, remain in WebView2's
original environment options.

Historical validity evidence for the NUC's WebView2 155: Chromium's
[milestone metadata](https://chromiumdash.appspot.com/fetch_milestones?mstone=155)
identifies branch 8059 and branch-point revision
`199ac2cd7ab289e11c2236bd2cede848781213da` (whose
[VERSION](https://chromium.googlesource.com/chromium/src/+/199ac2cd7ab289e11c2236bd2cede848781213da/chrome/VERSION)
has major 155). At that revision,
[ui_base_features.cc](https://chromium.googlesource.com/chromium/src/+/199ac2cd7ab289e11c2236bd2cede848781213da/ui/base/ui_base_features.cc)
defines Windows-only `CalculateNativeWinOcclusion`, and
[content_switches.cc](https://chromium.googlesource.com/chromium/src/+/199ac2cd7ab289e11c2236bd2cede848781213da/content/public/common/content_switches.cc)
defines `disable-backgrounding-occluded-windows` (a testing control),
`disable-renderer-backgrounding` (renderer priority) and
`disable-background-timer-throttling` (background timer tasks). Microsoft's
[WebView2 flag guidance](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/webview-features-flags)
and [AdditionalBrowserArguments reference](https://github.com/MicrosoftDocs/webview2-win32-reference/blob/main/1-0-3537-50/icorewebview2environmentoptions.md#get_additionalbrowserarguments)
describe passing Chromium switches through WebView2 for diagnostics; these flags
are development controls without long-term compatibility guarantees.

The same Chromium revision's
[native occlusion tracker](https://chromium.googlesource.com/chromium/src/+/199ac2cd7ab289e11c2236bd2cede848781213da/ui/aura/native_window_occlusion_tracker_win.cc)
marks windows occluded when the display is off or the session is locked. This
supports the display-off hypothesis for the NUC's blank screenshot/root timeout,
but does not establish that run's cause. The switches remove those Chromium
occlusion/background controls for this smoke process; a fresh Windows run with
the display off must establish the actual WebView2 155 result. No power/session
settings or ordinary app arguments are changed. There is no automatic
tauri-driver fallback.

**Actual profile isolation:** no `APPDATA`/`LOCALAPPDATA` overrides or deletion of
the runner user's normal `com.maxed.companion` directories. Each run creates a UUID directory
under `RUNNER_TEMP`, or `[IO.Path]::GetTempPath()` when unset. Child-only
`TAURI_CONFIG` during the build compiles this object into the executable:

```json
{
  "app": {
    "appDirectoriesOverride": {
      "config": "<profile>/app",
      "data": "<profile>/app",
      "localData": "<profile>/webview",
      "cache": "<profile>/cache",
      "log": "<profile>/logs"
    }
  }
}
```

Paths are absolute, JSON-encoded Windows paths. `com.maxed.companion` remains the
identifier, but these explicit paths replace its normal directories; it is not
appended again. The launcher also sets child-only `WEBVIEW2_USER_DATA_FOLDER` to
the same `<profile>/webview` and clears an inherited browser executable override.
The WebView2 environment API above documents this folder override. Tauri's default
WebView folder is already covered, so both mechanisms agree.

Exact locked source evidence (registry root on the implementation machine:
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`; these are dependency source
paths, not repository files):

| Source | Evidence |
| --- | --- |
| `Cargo.lock:4214`, `4265`, `4286`, `4430`, `4455`, `5672` | Tauri/runtime-wry 2.12.1; build/codegen 2.7.1; utils 2.10.1; Wry 0.57.0. |
| `tauri-utils-2.10.1/src/config.rs:3283`, `3396`, `3410`, `3507` | Camel-case `appDirectoriesOverride` exists; the object supports config/data/localData/cache/log, with exact-path semantics. The docs explicitly cover the default WebView data directory; an explicit window `dataDirectory` is the exception. |
| `tauri-build-2.7.1/src/lib.rs:656`, `682`; `tauri-codegen-2.7.1/src/lib.rs:85` | `TAURI_CONFIG` is merged at build/codegen time; Cargo reruns the build script when it changes. |
| `tauri-2.12.1/src/path/desktop.rs:251`, `265`; `src/path/mod.rs:410–437` | `app_data_dir()` and `app_local_data_dir()` resolve the configured overrides, before default Known Folder locations. |
| `tauri-2.12.1/src/manager/webview.rs:559–564` | Windows defaults the WebView's `data_directory` to `app_local_data_dir()`. The app's main-window config has no `dataDirectory` (`tauri.conf.json:14`). |
| `tauri-runtime-wry-2.12.1/src/lib.rs:4714`, `4737`; `wry-0.57.0/src/webview2/mod.rs:349–351` | The data directory reaches Wry's WebContext and WebView2's `userDataFolder` argument. |
| `Cargo.lock:835`, `844`; `dirs-7.0.0/src/win.rs:10–11`; `dirs-sys-0.5.0/src/lib.rs:150–177` | The default data directories use `SHGetKnownFolderPath`, not child `APPDATA` variables. |

Trade-off: this is a diagnostic custom-protocol debug build with a **per-run
compiled directory override**, not a plain debug executable. The warm Cargo target
is reused for dependencies, but changing the absolute profile path rebuilds the
app configuration and may relink the app each run. A fresh UUID avoids shared
profile reuse and any need to wipe a previous user's directories. The executable
retains the now-deleted profile path after cleanup; only this launcher should run it.

## Debug-only safety mode (pitfall 4)

`MAXED_GUI_SMOKE=1` is set on the app child only. The exact-match pure decision is
`app/src-tauri/src/gui_smoke.rs:3–4`. `enabled()` reads the variable once only under
`#[cfg(debug_assertions)]` (`:9–12`); the non-debug branch never reads it and returns
false (`:14–16`). Release builds with the repository's normal release profile ignore
the flag. Unit tests cover missing/malformed values and both debug-assertion modes.

The safety decision precedes window creation. `SetupService::new` skips Steam
roots (`setup.rs:205`); launcher-root resolution is skipped in `lib.rs:1037`, and
the service also refuses a supplied launcher root (`setup.rs:243`). Startup stays
on Setup (`setup.rs:253`). Setup queries/retries return the existing unchecked,
unknown-access, missing-assets status without starting the refresh worker
(`setup.rs:436`). Adapter enumeration and internal capture opening refuse
(`:284`, `:300`). The transition guard (`:271`) blocks capture, adapter changes,
settings-driven asset loading and demo; asset reservation also refuses (`:590`).
Read-only catalog commands retain their existing empty/unavailable responses.
Inherited demo/performance startup cannot bypass smoke mode (`lib.rs:1094`).

These guards are necessary: ordinary Setup queries call `refresh`, which opens
and stops a capture handle and searches game assets. The capture backend's
`probe()` loads Npcap (`setup.rs:32`; `crates/maxed-capture/src/lib.rs:20`). UI setup
mounting triggers these queries (`app/ui/src/routes/Setup.svelte:15`,
`app/ui/src/lib/setup/session.ts:138–141`). Smoke tests must not rely on Npcap merely
being absent. Service unit tests use a backend that panics on probe/enumeration/
capture to catch accidental calls; the production guard returns ordinary errors.

## UI/error checks and cleanup (pitfall 5)

An empty `#app` from `app/ui/index.html:10` is insufficient. The script requires
the rendered `.app-frame`, `#main-content` (`Shell.svelte:511,524`) and the visible
Setup list with three steps (`routes/Setup.svelte:62`). `src/main.ts:29` mounts the
real App. Discovery accepts only page targets at the bundled Windows Tauri origin
and root/index path, excluding workers and devtools; multiple matching pages fail.

Event handlers are installed before enabling Runtime/Log/Network/Page. The script
retains any error events replayed at attach, installs `error`/`unhandledrejection` handlers
with `Page.addScriptToEvaluateOnNewDocument`, then calls `Page.navigate` with the
validated target's origin plus `/`. WebView2 discovery can advertise the pending
app URL while the committed document is still `about:blank`; reloading then would
abort the app navigation and reload the blank document. Explicit navigation loads
the app after instrumentation is installed. Before polling the root, the script
waits within the shared deadline for a main-frame `Page.frameNavigated` at that
same origin with the frame and loader IDs returned by `Page.navigate`. Events
arriving before the navigate response are retained; subframes and stale loaders
do not satisfy the wait. A navigate `errorText` (including `net::ERR_ABORTED`) fails
with that text, without retry. A main-frame commit to any other origin fails with
its URL, including during observation.

A marker proves the instrumented document mounted. After that it requires ten
seconds with no
`Runtime.exceptionThrown`, `Runtime.consoleAPICalled` of type `error`,
`Log.entryAdded` of level `error`, or captured early document errors. Warnings are
allowed. Historical initial-page errors that CDP does not replay cannot be
recovered; the instrumented navigation supplies startup coverage. Any such error,
missing/disappearing root, malformed response or deadline expiry fails the smoke.

Before waiting for the root, the script requests `Page.bringToFront` and
`Emulation.setFocusEmulationEnabled` (`enabled: true`), each bounded to one second
within the run deadline. Unsupported/failed hints are harmless and retained as
`diagnostics.focus_errors` on failure. Focus emulation can affect the page's
reported focus/visibility; those fields describe its effective JS state, not
proof that the physical monitor is on or the Windows session is unlocked.

On a rendered-root timeout or any CDP failure after attach, `report.json` includes
`diagnostics` before attempting the failure screenshot:

- `page.location.href`; `page.document.readyState`, `visibilityState`, `hidden`
  and `hasFocus`; `page.window.innerWidth` and `innerHeight`.
- `page.elements.app_frame`, `main_content` and `setup_list`: existence, a rect
  (`x`, `y`, `top`, `right`, `bottom`, `left`, `width`, `height`) and, when present,
  element-child count. Missing elements have `exists: false, rect: null`.
- `page.body.childElementCount` and the first 300 characters of `innerText`.
  A missing body has count zero and empty text. Only the temporary synthetic
  Setup profile is inspected.
- `console_messages` at every level, `network_loading_failed` from
  `Network.loadingFailed`, and `log_entries` from `Log.entryAdded`: the last 20
  of each, in arrival order with CDP timestamps. Fields are selected explicitly;
  each string is capped at 1,000 characters (document state strings at 100), and
  console argument summaries use at most 20 arguments. Rects/counts/timestamps
  are finite numbers or null; no unbounded stack/object dumps are kept.

If page evaluation fails or CDP disconnects, `page` is null and `page_error`
explains why; event histories already seen are retained. Diagnostic/screenshot
failures cannot replace the original `reason`. Network failures and non-error
console/log entries are evidence only; the existing error policy still applies.
A loaded document with missing roots/text suggests JS/mount trouble; `loading`
or an unexpected URL suggests navigation/custom-protocol trouble; zero rects or
hidden state suggest rendering/visibility trouble. These are clues, not a
classification that overrides the failure.

`WindowsJob.cs` creates the child suspended, assigns it to a Windows Job Object,
then resumes it. The job has `KILL_ON_JOB_CLOSE`, no breakaway flag, and includes
the Node probe. In `finally`, the launcher first requests `CloseMainWindow` and
allows up to three seconds for Maxed to exit. Whether it exits, refuses the close,
or times out, `TerminateJobObject` then runs and waits up to ten seconds for the
job's active process count to reach zero. Descendants are covered even when the
parent has already exited; there is no process-name-wide kill. Assignment failure
terminates the still-suspended child and fails closed.

Profile deletion has a 30-second retry budget with exponential delays from 250 ms
to two seconds. Each failed attempt queries Windows Restart Manager using
`RmStartSession`, `RmRegisterResources`, `RmGetList` and `RmEndSession`. Remaining
files are registered as one batch: the deletion exception may supply only a leaf
name, and Restart Manager does not accept directories. The job handle stays open
through diagnostics so `IsProcessInJob` can test membership in **this job** even
after termination. The outer `finally` always disposes the job; kill-on-close is
still the final guarantee. `Dispose` also terminates/waits if termination has not
already succeeded. No Restart Manager shutdown/restart API is called, and no
outside process is killed.

Every failed deletion writes a `profile_lockers` sample to `launcher.json` and a
`GUI smoke profile lock diagnostic:` JSON line to stdout. Samples contain elapsed
milliseconds, the deletion error, registered file paths, `processes`, and any
diagnostic error. Stdout also confirms zero active job processes and gives elapsed
time when deletion succeeds. Each process contains:

- `process_id`, `image_name`, `application_name` (Restart Manager's display name),
  and `started_utc` from Restart Manager's PID/start-time identity.
- `alive` and `in_job`: nullable booleans; null means inspection was unavailable,
  not proof the process exited or was outside the job.
- `diagnostic_error`: access/inspection failure or a PID-reuse explanation.

Inspection opens a handle with query/synchronize rights, verifies the creation
time against Restart Manager, and checks liveness and membership on that handle.
It never attributes a recycled PID to the original locker. An exited process may
have `image_name: "<unavailable>"`; its Restart Manager identity is preserved.
API failures are logged rather than reported as an empty successful query. The
retry budget includes diagnostic time, but a synchronous Windows API call can
overrun the deadline; retries do not continue indefinitely after it returns.

A verified live locker in the terminated job produces an **ORPHAN** cleanup
failure; a live outside locker produces a failure naming its image, PID and start
time. Unknown membership on a live locker also fails. These failures remain even
if a later deletion succeeds. A lock that clears without a live locker detected
can pass, with all retry evidence retained. Persistent deletion failure always
returns non-zero, including when Restart Manager fails or finds no processes.
An empty list is not proof of delayed handle release: directory-only handles,
kernel activity and inspection races may not be attributable by Restart Manager.

NUC run `38627574099` passed its real WebView2 CDP checks but failed deleting
`Extension Rules/000003.log` after the job reached zero active processes. That
evidence alone cannot distinguish delayed release, an outside WebView2/broker
process, or a scanner. The next run's samples will show which live processes
Restart Manager observes and their job membership; retry timing will show whether
the lock clears. A run with no deletion failure will have an empty diagnostics
array and cannot establish the original cause.

The workflow also runs `Invoke-GuiSmoke.ps1 -CleanupOnly -OutputDir <same-directory>`
with `if: always()` before upload. This reads `owner.json`, retains a handle to that
specific PID, and compares its start time and executable before terminating it.
A missing record, exited launcher or recycled PID is left alone. If the launcher
survived cancellation, terminating it closes its sole non-inheritable Job Object
handle, which kills its app/WebView2/Node descendants. It never searches processes
by name. A forced launcher termination can bypass profile deletion; that profile
remains under the runner's temporary directory for runner cleanup. Normal exit
still uses the launcher's `finally` to terminate the job and delete the profile.

## Offline verification and remaining Windows checks

```text
node --test tests/gui-smoke/*.test.mjs
pwsh -NoProfile -File tests/gui-smoke/Invoke-GuiSmoke.test.ps1
pwsh -NoProfile -File tests/packaging/workflow-policy.test.ps1
cargo fmt --all -- --check
git diff --check main...HEAD
```

The Node suite uses a real local `/json` HTTP server and an injected WebSocket
transport, including a real ten-second default observation. Node 22.16.0 expands
the explicit `*.test.mjs` argument itself (also verified with a quoted glob), so
the old `index.js` directory-resolution shim has been removed. It also evaluates
the real diagnostic expression against synthetic DOM state, checks bounded
event rings, foreground/focus hints, and failure evidence after deadline expiry
or disconnect. Navigation mocks cover a pending app URL with committed
`about:blank`, same-origin root normalization, navigate errors, foreign main-frame
commits, matching loader waits in either event/response order, and errors from the
first app script after the real early-error hooks run. The PowerShell suite checks
the port/occlusion switches exactly once, default-feature preservation, duplicate-feature merging and rejection of
unrecognized defaults. It uses mocks for build and application launch, verifies first-PATH selection with
multiple synthetic command matches, rejects invalid/missing paths, exercises a
real port collision, and compiles the C# helper without invoking Windows APIs. A synthetic sleeper
tests cleanup's PID/start-time identity guard and termination on every platform.
Mocks at the deletion and Restart Manager boundaries exercise retry-to-success,
persistent outside/in-job lockers, diagnostic failure, retained locker evidence,
stdout and report output, and graceful-close timeout/error fallback to forced
termination. A later successful deletion cannot mask a detected live locker.
On Windows it also checks native launch failure diagnostics, opens a synthetic
locked file in a job-owned process to verify Restart Manager marshaling, process
identity and exact job membership, and launches a synthetic Node parent/child to
verify orphan cleanup by the actual job.

macOS validation can run the pure Rust module independently using `rustc --test
app/src-tauri/src/gui_smoke.rs` in a temporary output location, with default flags
and with `-C debug-assertions=no`. `cargo check --locked -p maxed-types` is a
portable sanity check, not validation of `maxed-app`. The service unit test runs
with `cargo test --locked --workspace` on Windows. Full app compilation, service
tests and the new native Restart Manager coverage require Windows. The earlier
NUC CDP/screenshot success does not validate these cleanup changes; a fresh
GitHub-hosted `windows-2022` run is required. The display-independent smoke
changes also need validation in that hosted session, including a deliberately
failed mount to inspect the new report. These hosted checks remain unverified. No
live Npcap, game-client or installer behavior is claimed.

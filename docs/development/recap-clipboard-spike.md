# Recap clipboard spike — 2026-10-13

The PNG clipboard path passed in the real Windows WebView2 app at
`http://tauri.localhost`. This used a release executable with `custom-protocol`,
an isolated native data directory and an isolated WebView2 profile. The owner
approved this machine's run and one clipboard overwrite on October 13, 2026.

## Observed results

- Time: `2026-10-13T15:16:22.743Z`.
- `window.isSecureContext`: `true`.
- `navigator.clipboard.write` and `ClipboardItem`: available.
- A 64×32 canvas filled with `#7b5cd6` produced an `image/png` blob (183 bytes).
- `navigator.clipboard.write([new ClipboardItem({ 'image/png': blob })])` resolved.
- Windows Forms `Clipboard.GetImage()`, called from a PowerShell STA process,
  returned a 64×32 image. Pixel (1,1) was `#7b5cd6`, alpha 255.
- There were zero `img[src^="http://maxed-sprite.localhost/"]` elements in the
  fresh-profile page: **sprite path not exercised live**.
- The app exited and the isolated profile was deleted after verification.

Runtime `navigator.userAgent`:

```text
Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/154.0.0.0 Safari/537.36 Edg/154.0.0.0
```

The tested executable's SHA-256 was
`4640a21b01d2c0e6fe3a51f0314fe66f12c149a4fc24924d05df1b3457549a26`.
It was built from `c66f948d` plus this task's sprite CORS changes, with Rust 1.99.0
and Node.js 22.16.0. This was an unpackaged release executable, not an installer test.

## Commands and isolation

The scratch scripts and local logs are private and are not committed.
The examples below use `<private-spike-directory>` as a placeholder:

```powershell
npm.cmd --prefix app/ui ci
powershell -NoProfile -ExecutionPolicy Bypass -File <private-spike-directory>/build.ps1
powershell -STA -NoProfile -ExecutionPolicy Bypass -File <private-spike-directory>/run.ps1
```

The build script used the absolute-identifier technique from
[the live realm attribution report](../performance/2026-10-06-live-realm-attribution.md).
Its build operations were:

```powershell
$spikeProfile = [IO.Path]::GetFullPath(
    (Join-Path (Get-Location) '<private-spike-directory>/profile')
)
$env:TAURI_CONFIG = @{
    identifier = $spikeProfile
    app = @{ windows = @(@{
        label='main'; create=$false; title='Maxed'
        width=1100; height=760; minWidth=800; minHeight=560
        dataDirectory=(Join-Path $spikeProfile 'webview')
    }) }
} | ConvertTo-Json -Depth 6 -Compress
npm.cmd --prefix app/ui run build
if (Get-Process cargo,rustc -ErrorAction SilentlyContinue) {
    throw 'Cargo or rustc already running'
}
$env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER = 'link.exe'
cargo build --locked --offline --release -p maxed-app --features maxed-app/custom-protocol
```

The script restored the previous build environment afterwards. Before launching,
the driver checked the executable hash and the literal isolated profile path in
the compiled bytes. It required a fresh profile and a free debugging port, cleared
the demo/performance and WebView user-data environment overrides, and launched with:

```powershell
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9239'
$process = Start-Process -FilePath $exe -PassThru -WindowStyle Hidden `
    -RedirectStandardError (Join-Path $spikeRoot 'app-stderr.log') `
    -RedirectStandardOutput (Join-Path $spikeRoot 'app-stdout.log')
node (Join-Path $spikeRoot 'clipboard.mjs')
```

The Node script reused the `connect` helper from
`tests/performance/live-realm-attribution.mjs`. It selected the app page from
`http://127.0.0.1:9239/json/list`, called `Page.bringToFront`, then used
`Runtime.evaluate` with `userGesture: true`, `awaitPromise: true` and
`returnByValue: true`. It read the context and user agent before making exactly
one clipboard write. The evaluated clipboard operation was:

```javascript
const canvas = document.createElement('canvas');
canvas.width = 64;
canvas.height = 32;
const ctx = canvas.getContext('2d');
ctx.fillStyle = '#7b5cd6';
ctx.fillRect(0, 0, 64, 32);
const blob = await new Promise(resolve => canvas.toBlob(resolve, 'image/png'));
if (!blob) throw new Error('Canvas toBlob returned null');
await navigator.clipboard.write([new ClipboardItem({ 'image/png': blob })]);
```

After the write, the script checked for an existing sprite image. Its conditional
sprite probe sets `new Image().crossOrigin = 'anonymous'` before assigning the
existing sprite URL, draws the loaded image into a canvas and calls `toBlob`.
That branch did not run because no matching image was present.

The STA driver verified the native clipboard without saving its contents:

```powershell
Add-Type -AssemblyName System.Windows.Forms,System.Drawing
$clipboardImage = [System.Windows.Forms.Clipboard]::GetImage()
if ($null -eq $clipboardImage) { throw 'Windows clipboard holds no image' }
$pixel = $clipboardImage.GetPixel(1, 1)
$color = '#{0:x2}{1:x2}{2:x2}' -f $pixel.R,$pixel.G,$pixel.B
# Verified Width=64, Height=32, color='#7b5cd6', A=255.
$clipboardImage.Dispose()
```

Cleanup ran in `finally`: close the owned app, stop any remaining owned children,
verify the resolved profile path remains under the scratch spike directory with
no reparse-point ancestors, then `Remove-Item -LiteralPath $spikeProfile -Recurse -Force`.
The driver recorded `app_exited: true` and `profile_deleted: true`.

## Scope and remaining evidence

The spike overwrote the clipboard once with the synthetic test image. It did not
read or restore the previous clipboard contents. Neither clipboard image bytes
nor game sprites are committed. The real `%APPDATA%\com.maxed.companion` data was
never read or written. Start was never clicked and capture was never started.

The sprite response now sends `Access-Control-Allow-Origin: <origin>` and
`Vary: Origin` only for status 200 and an exact allowed origin:
`http://tauri.localhost`, or `http://127.0.0.1:1420` in debug builds. Missing,
foreign and failed requests receive neither header. Later recap code must set
`img.crossOrigin = 'anonymous'` before assigning the sprite URL.

Synthetic response tests establish those header rules. This spike establishes
the real WebView2 canvas-to-PNG-to-Windows-clipboard path for the solid test
canvas; the live sprite-to-canvas step still needs evidence when sprites are
present. No clipboard plugin or dependency was added.

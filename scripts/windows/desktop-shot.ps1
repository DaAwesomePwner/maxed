[CmdletBinding()]
param([Parameter(Mandatory)][string]$OutputPath)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) { throw 'Desktop capture requires Windows' }
$path = [IO.Path]::GetFullPath($OutputPath)
$target = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../target')) + [IO.Path]::DirectorySeparatorChar
if (-not $path.StartsWith($target, [StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetExtension($path) -ine '.png') {
    throw 'Desktop screenshots must be PNGs under this worktree target/ directory'
}
# Reject junctions/symlinks on the destination's ancestors even when invoked
# directly, rather than through the Node driver's realpath check.
$parent = Get-Item -LiteralPath ([IO.Path]::GetDirectoryName($path))
while ($null -ne $parent) {
    if ($parent.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Screenshot ancestors must not be reparse points' }
    $parent = $parent.Parent
}
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type 'using System.Runtime.InteropServices; public class ReviewDesktopDpi { [DllImport("user32.dll")] public static extern bool SetProcessDPIAware(); }'
[ReviewDesktopDpi]::SetProcessDPIAware() | Out-Null
$rect = [System.Windows.Forms.SystemInformation]::VirtualScreen
$bitmap = [Drawing.Bitmap]::new($rect.Width, $rect.Height)
try {
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    try { $graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, $rect.Size) }
    finally { $graphics.Dispose() }
    $stream = [IO.File]::Open($path, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write)
    try { $bitmap.Save($stream, [Drawing.Imaging.ImageFormat]::Png) }
    finally { $stream.Dispose() }
} finally { $bitmap.Dispose() }
@{ path = $path; width = $rect.Width; height = $rect.Height; bytes = (Get-Item -LiteralPath $path).Length } | ConvertTo-Json -Compress

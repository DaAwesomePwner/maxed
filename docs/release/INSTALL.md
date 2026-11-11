# Install Maxed

## Requirements

Use Windows 10 or 11, x64, and the stock Realm of the Mad God client.
Maxed installs for the current Windows user. Run Maxed without elevation.
Npcap is a separate dependency; its installer may need administrator approval.

These instructions describe the configured 0.3.0 installer. Clean-machine results,
including exact Windows prompt text, must be recorded in the [release checklist](CHECKLIST.md).

## Check the download

Use a versioned installer from [Maxed releases](https://github.com/DaAwesomePwner/maxed/releases). See the [update guide](UPDATES.md) for release and channel policy.
Public downloads are not ready until maintainers complete the public-release gates.
Check the repository origin and release version, then compare the installer name,
byte count and SHA-256 with that release's `release-manifest.json` before running it.
In PowerShell, run `Get-FileHash -Algorithm SHA256` followed by the quoted path to
the downloaded installer. Every hash character must match. Stop if it does not.

The Windows executable and installer are not Authenticode-signed. SmartScreen
or an unknown-publisher prompt may appear. Update signatures are separate from
Windows publisher signing and do not remove these prompts. Do not disable
SmartScreen or Defender. If policy blocks the installer, leave the block in place
and ask your administrator. Exact prompt wording is to be verified in the checklist.

## Install dependencies and Maxed

1. Install Npcap separately from the [official Npcap download page](https://npcap.com/#download).
   Use its default mode. Leave **Restrict Npcap driver's access to Administrators only** unchecked.
   If Npcap is already restricted, rerun its installer and untick that option.
   Elevating Maxed is not a repair.
2. Run the verified Maxed installer. Its finish page gives Npcap guidance; it does
   not download or launch Npcap for you.
3. If WebView2 is absent, the installer is configured to download and run Microsoft's
   bootstrapper. An existing runtime skips that download. Without internet access,
   a missing runtime cannot be bootstrapped. Download or execution failure aborts
   installation. Restore connectivity and rerun the installer. Offline, cancel,
   failure and repair behavior still require clean-machine Windows verification.

## First launch

Open Maxed normally. Setup checks Npcap compatibility, access, the selected network
adapter and live traffic. Select your local game assets if discovery needs help.
After fixing a dependency or selection, choose **Retry checks** in Setup.
Demo and history work without Npcap; live capture needs it.

## Updates

Checks, downloads and installation are separate manual actions in Settings.
Stable is the default; beta requires opt-in. Returning to stable does not downgrade
a newer beta. See [channels and update delivery](UPDATES.md) for the release policy.
Installed-update acceptance is a release gate; a signed build alone does not establish a public release.

## Repair, reinstall and removal

Rerun a verified same-version or newer installer to repair program files.
Retention of settings, history, extracted assets and recordings across repair,
update and uninstall is expected, to be verified in [CHECKLIST](CHECKLIST.md).
Uninstall is expected to remove program files, shortcuts and registration while
keeping user data and shared dependencies such as Npcap and WebView2.

Maxed's application data is in `%APPDATA%\com.maxed.companion\`.
See [local data and privacy](../../PRIVACY.md) before sharing or deleting it.
To remove this data yourself, close Maxed, uninstall it, then enter that exact
folder in File Explorer and delete it. This permanently removes its settings,
history, recordings and extracted cache. Back up anything you want to keep first.

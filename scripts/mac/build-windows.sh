#!/usr/bin/env bash
# Cross-builds the Windows app on the Mac for live testing on the Windows box.
# Output: target/maxed-win-x64/ and target/maxed-win-x64-<sha>[-dirty].zip (exe, CLI, PDBs, demo, notices).
# Dev/test artifact only: lld-link + xwin CRT, unsigned, no NSIS. Releases still come from
# scripts/build-release.ps1 on Windows.
set -euo pipefail
cd "$(dirname "$0")/../.."
. scripts/mac/env.sh
[ "$(node -p process.version)" = v22.16.0 ] || { echo "Node 22.16.0 required (CI pin); see scripts/mac/env.sh" >&2; exit 1; }
WIN=x86_64-pc-windows-msvc
REL="target/$WIN/release"
OUT=target/maxed-win-x64
SHA="$(git rev-parse --short HEAD)"
DIRTY="$(git status --porcelain | head -1)"
NAME="maxed-win-x64-$SHA${DIRTY:+-dirty}"

# Same frontend + feature switch as build-release.ps1; plain builds point at the dev server.
npm --prefix app/ui run build
PATH="$MAXED_XWIN_PATH:$PATH" cargo xwin build --release --locked -p maxed-app -p maxed-cli \
  --features maxed-app/custom-protocol --target "$WIN"

for f in demo/story.pcap demo/catalog.json; do
  cmp -s "$REL/$f" "tests/fixtures/scrubbed/maxed-demo/${f#demo/}" || { echo "Bundled $f does not match the synthetic source." >&2; exit 1; }
done

rm -rf "$OUT" "$OUT"-*.zip
mkdir -p "$OUT/demo"
cp "$REL"/maxed.exe "$REL"/maxed.pdb "$REL"/maxed-cli.exe "$REL"/maxed_cli.pdb "$OUT/"
cp "$REL"/LICENSE "$REL"/NOTICE.md "$REL"/THIRD-PARTY-NOTICES.md "$OUT/"
cp "$REL"/demo/story.pcap "$REL"/demo/catalog.json "$OUT/demo/"
{
  echo "branch: $(git rev-parse --abbrev-ref HEAD)"
  echo "commit: $(git rev-parse HEAD)${DIRTY:+ (dirty working tree)}"
  echo "built:  $(date -u +%Y-%m-%dT%H:%M:%SZ) on macOS via cargo-xwin"
} > "$OUT/BUILD-INFO.txt"
(cd target && zip -qr "$NAME.zip" maxed-win-x64)
echo "Windows build: $PWD/target/$NAME.zip"

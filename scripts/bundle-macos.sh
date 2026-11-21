#!/usr/bin/env bash
# Wrap a release binary in an app bundle and zip it.
# Usage: scripts/bundle-macos.sh <binary> <output dir> [entitlements.plist]
# With entitlements the bundle is ad-hoc signed with them, so the sandbox can
# be tried locally; release signing happens elsewhere.
set -euo pipefail

binary="$1"
out="$2"
entitlements="${3:-}"
here="$(cd "$(dirname "$0")/.." && pwd)"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$here/Cargo.toml" | head -1)"
app="$out/aturfile.app"

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources/Licenses"
cp "$binary" "$app/Contents/MacOS/aturfile"
sed "s/@VERSION@/$version/g" "$here/packaging/Info.plist" > "$app/Contents/Info.plist"

if [[ -f "$here/packaging/AppIcon.png" ]]; then
  iconset="$(mktemp -d)/AppIcon.iconset"
  mkdir -p "$iconset"
  for size in 16 32 128 256 512; do
    sips -z "$size" "$size" "$here/packaging/AppIcon.png" --out "$iconset/icon_${size}x${size}.png" >/dev/null
    sips -z $((size * 2)) $((size * 2)) "$here/packaging/AppIcon.png" --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
  done
  iconutil -c icns "$iconset" -o "$app/Contents/Resources/AppIcon.icns"
  /usr/libexec/PlistBuddy -c "Add :CFBundleIconFile string AppIcon" "$app/Contents/Info.plist"
fi

# Licence texts travel with every copy.
cp "$here/LICENSE" "$app/Contents/Resources/Licenses/LICENSE"
cp "$here"/packaging/licenses/*.txt "$app/Contents/Resources/Licenses/"

if [[ -n "$entitlements" ]]; then
  codesign --force --sign - --entitlements "$entitlements" "$app"
else
  codesign --force --deep --sign - "$app"
fi

arch="$(lipo -archs "$app/Contents/MacOS/aturfile" | tr ' ' '-')"
suffix="${entitlements:+-sandboxed}"
(cd "$out" && ditto -c -k --keepParent "aturfile.app" "aturfile-${version}-macos-${arch}${suffix}.zip")
echo "Bundled $app ($version, $arch${suffix})"

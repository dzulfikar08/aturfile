#!/usr/bin/env bash
# Mac App Store build of aturfile: universal (arm64 + x86_64), `store`
# feature (StoreKit 2 Pro unlock), App Sandbox. Release steps: docs/release.md.
#
#   scripts/build-mas.sh bundle <out dir>                   # compile, lipo, bundle: <out>/aturfile.app (ad hoc)
#   scripts/build-mas.sh package <app> <out dir> [--upload] # sign, pack <out>/aturfile-<version>.pkg,
#                                                           # validate with App Store Connect (or upload)
#
# `bundle` needs no secrets: CI builds it on every push so the store variant
# can't rot. `package` needs the "Apple Distribution" and "3rd Party Mac
# Developer Installer" identities in the keychain, MAS_PROFILE (a Mac App
# Store provisioning profile for BUNDLE_ID) and, to validate or upload,
# ASC_KEY_ID and ASC_ISSUER_ID (altool reads
# ~/.appstoreconnect/private_keys/AuthKey_<id>.p8). BUILD_NUMBER
# (CFBundleVersion) defaults to the Unix time.
set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
# Placeholder until the bundle id is registered (docs/release.md).
BUNDLE_ID="${BUNDLE_ID:-com.atur.aturfile}"
usage="usage: $0 bundle <out dir> | package <app> <out dir> [--upload]"
# StoreKit 2 and the Swift concurrency runtime built into macOS (see
# crates/af-store/build.rs); matches LSMinimumSystemVersion.
export MACOSX_DEPLOYMENT_TARGET=13.0

identity() { # SHA-1 of the first valid identity whose name contains $1 ($2: optional -p policy)
  security find-identity -v ${2:+-p "$2"} | awk -v n="$1" 'index($0, n) { print $2; exit }'
}
identity_name() { # full name of that identity (productbuild wants a name)
  security find-identity -v | awk -F'"' -v n="$1" 'index($0, n) { print $2; exit }'
}

case "${1:-}" in
bundle)
  out="${2:?$usage}"
  mkdir -p "$out"
  out="$(cd "$out" && pwd)"
  binaries=()
  for target in aarch64-apple-darwin x86_64-apple-darwin; do
    (cd "$here" && cargo build --locked --release -p af-app --features store --target "$target")
    binaries+=("$here/target/$target/release/aturfile")
  done
  lipo -create -output "$out/aturfile" "${binaries[@]}"
  archs="$(lipo -archs "$out/aturfile")"
  [[ "$archs" == *x86_64* && "$archs" == *arm64* ]] || { echo "not a universal binary: $archs" >&2; exit 1; }

  # Sandboxed and signed ad hoc with the store entitlements, so it launches
  # for a smoke test; `package` signs it for real.
  "$here/scripts/bundle-macos.sh" "$out/aturfile" "$out" "$here/packaging/entitlements.mas.plist"
  rm -f "$out/aturfile" "$out"/aturfile-*-macos-*.zip
  app="$out/aturfile.app"
  plist="$app/Contents/Info.plist"
  put() { # key type value: set, or add when missing
    /usr/libexec/PlistBuddy -c "Set :$1 $3" "$plist" 2>/dev/null \
      || /usr/libexec/PlistBuddy -c "Add :$1 $2 $3" "$plist"
  }
  put CFBundleIdentifier string "$BUNDLE_ID"
  put CFBundleVersion string "${BUILD_NUMBER:-$(date +%s)}"
  codesign --force --sign - --entitlements "$here/packaging/entitlements.mas.plist" "$app"
  version="$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" "$plist")"
  (cd "$out" && ditto -c -k --keepParent "aturfile.app" "aturfile-$version-mas-unsigned.zip")
  echo "==> $app ($archs)"
  ;;

package)
  app="${2:?$usage}"
  out="${3:?$usage}"
  flag="${4:-}"
  [[ -z "$flag" || "$flag" == --upload ]] || { echo "$usage" >&2; exit 1; }
  : "${MAS_PROFILE:?MAS_PROFILE must point to a Mac App Store provisioning profile for $BUNDLE_ID}"
  mkdir -p "$out"
  APP_ID="$(identity "Apple Distribution" codesigning)"
  PKG_ID="$(identity_name "3rd Party Mac Developer Installer")"
  [[ -n "$APP_ID" && -n "$PKG_ID" ]] || { echo "Apple Distribution / Mac Installer identity missing" >&2; exit 1; }
  plist="$app/Contents/Info.plist"
  [[ "$(/usr/libexec/PlistBuddy -c "Print :CFBundleIdentifier" "$plist")" == "$BUNDLE_ID" ]] \
    || { echo "$app is not a store bundle (scripts/build-mas.sh bundle)" >&2; exit 1; }

  # codesign doesn't take entitlements from the profile: the application and
  # team identifiers must be declared and match it (App Store Connect 90886).
  profile_plist="$out/profile.plist"
  security cms -D -i "$MAS_PROFILE" > "$profile_plist"
  aid="$(/usr/libexec/PlistBuddy -c "Print :Entitlements:com.apple.application-identifier" "$profile_plist")"
  team="$(/usr/libexec/PlistBuddy -c "Print :Entitlements:com.apple.developer.team-identifier" "$profile_plist")"
  [[ "$aid" == "$team.$BUNDLE_ID" ]] || { echo "profile is for $aid, expected $team.$BUNDLE_ID" >&2; exit 1; }
  ent="$out/entitlements.plist"
  cp "$here/packaging/entitlements.mas.plist" "$ent"
  /usr/libexec/PlistBuddy -c "Add :com.apple.application-identifier string $aid" "$ent"
  /usr/libexec/PlistBuddy -c "Add :com.apple.developer.team-identifier string $team" "$ent"

  cp "$MAS_PROFILE" "$app/Contents/embedded.provisionprofile"
  xattr -cr "$app"
  codesign --force --options runtime --timestamp --entitlements "$ent" --sign "$APP_ID" "$app"
  codesign --verify --strict --verbose=2 "$app"
  codesign -d --entitlements - "$app"
  version="$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" "$plist")"
  pkg="$out/aturfile-$version.pkg"
  productbuild --component "$app" /Applications --sign "$PKG_ID" "$pkg"
  echo "==> $pkg"
  if [[ "$flag" == --upload ]]; then
    xcrun altool --upload-app --type macos --file "$pkg" --apiKey "$ASC_KEY_ID" --apiIssuer "$ASC_ISSUER_ID"
  elif [[ -n "${ASC_KEY_ID:-}" ]]; then
    xcrun altool --validate-app --type macos --file "$pkg" --apiKey "$ASC_KEY_ID" --apiIssuer "$ASC_ISSUER_ID"
  fi
  ;;

*)
  echo "$usage" >&2
  exit 1
  ;;
esac

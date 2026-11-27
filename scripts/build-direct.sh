#!/usr/bin/env bash
# Direct-download (Developer ID) build of aturfile: universal (arm64 + x86_64),
# no `store` feature, no App Sandbox, notarised. It updates itself
# (crates/af-update); the release workflow is .github/workflows/direct.yml.
#
#   scripts/build-direct.sh bundle <out dir>      # compile, lipo, bundle: <out>/aturfile.app (ad hoc)
#   scripts/build-direct.sh sign <app>            # Developer ID, hardened runtime, timestamp
#   scripts/build-direct.sh notarize <app>        # notarytool, staple, Gatekeeper check
#   scripts/build-direct.sh package <app> <out>   # <out>/aturfile-X.Y.Z-universal-macos.tar.gz (the
#                                                 # update) and aturfile-X.Y.Z-macos.zip (for people)
#
# `sign` needs a "Developer ID Application" identity in the keychain;
# `notarize` needs ASC_KEY_PATH, ASC_KEY_ID and ASC_ISSUER_ID (scripts/ci-keychain.sh).
set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
usage="usage: $0 bundle <out> | sign <app> | notarize <app> | package <app> <out>"
# Matches LSMinimumSystemVersion in packaging/Info.plist.
export MACOSX_DEPLOYMENT_TARGET=13.0

version_of() { /usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" "$1/Contents/Info.plist"; }

case "${1:-}" in
bundle)
  out="${2:?$usage}"
  mkdir -p "$out"
  out="$(cd "$out" && pwd)"
  binaries=()
  for target in aarch64-apple-darwin x86_64-apple-darwin; do
    (cd "$here" && cargo build --locked --release -p af-app --target "$target")
    binaries+=("$here/target/$target/release/aturfile")
  done
  lipo -create -output "$out/aturfile" "${binaries[@]}"
  archs="$(lipo -archs "$out/aturfile")"
  [[ "$archs" == *x86_64* && "$archs" == *arm64* ]] || { echo "not a universal binary: $archs" >&2; exit 1; }
  "$here/scripts/bundle-macos.sh" "$out/aturfile" "$out"
  rm -f "$out/aturfile" "$out"/aturfile-*-macos-*.zip
  echo "==> $out/aturfile.app ($archs)"
  ;;

sign)
  app="${2:?$usage}"
  identity="$(security find-identity -v -p codesigning | awk -F'"' '/Developer ID Application/ { print $2; exit }')"
  [[ -n "$identity" ]] || { echo "no Developer ID Application identity in the keychain" >&2; exit 1; }
  # Hardened runtime (required for notarisation), secure timestamp. Plain
  # Rust with no JIT and no nested code: no entitlements, no --deep.
  xattr -cr "$app"
  codesign --force --options runtime --timestamp --sign "$identity" "$app"
  codesign --verify --strict --verbose=2 "$app"
  codesign --display --verbose=2 "$app" 2>&1 | grep -E '^(Authority|TeamIdentifier|Identifier|Runtime Version|Timestamp)=' || true
  ;;

notarize)
  app="${2:?$usage}"
  : "${ASC_KEY_PATH:?}" "${ASC_KEY_ID:?}" "${ASC_ISSUER_ID:?}"
  work="$(mktemp -d)"
  trap 'rm -rf "$work"' EXIT
  ditto -c -k --keepParent "$app" "$work/aturfile.zip"
  # notarytool exits 0 for a finished but rejected submission: read the status.
  xcrun notarytool submit "$work/aturfile.zip" --key "$ASC_KEY_PATH" --key-id "$ASC_KEY_ID" \
    --issuer "$ASC_ISSUER_ID" --wait --timeout 1h --output-format json > "$work/result.json" || true
  cat "$work/result.json"; echo
  status="$(plutil -extract status raw -o - "$work/result.json" 2>/dev/null || true)"
  sub_id="$(plutil -extract id raw -o - "$work/result.json" 2>/dev/null || true)"
  if [[ "$status" != Accepted ]]; then
    echo "error: notarisation status '${status:-unknown}'" >&2
    if [[ -n "$sub_id" ]]; then
      xcrun notarytool log "$sub_id" --key "$ASC_KEY_PATH" --key-id "$ASC_KEY_ID" \
        --issuer "$ASC_ISSUER_ID" >&2 || true
    fi
    exit 1
  fi
  xcrun stapler staple "$app"
  xcrun stapler validate "$app"
  spctl --assess --type execute --verbose=2 "$app"
  ;;

package)
  app="${2:?$usage}"
  out="${3:?$usage}"
  mkdir -p "$out"
  version="$(version_of "$app")"
  name="aturfile-$version-universal-macos"
  stage="$(mktemp -d)"
  trap 'rm -rf "$stage"' EXIT
  mkdir "$stage/$name"
  ditto "$app" "$stage/$name/aturfile.app"
  tar -czf "$out/$name.tar.gz" -C "$stage" "$name"
  ditto -c -k --keepParent "$app" "$out/aturfile-$version-macos.zip"
  echo "==> $out/$name.tar.gz, $out/aturfile-$version-macos.zip"
  ;;

*)
  echo "$usage" >&2
  exit 1
  ;;
esac

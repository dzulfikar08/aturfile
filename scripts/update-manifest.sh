#!/usr/bin/env bash
# update-manifest.sh VERSION NOTES_URL BASE_URL OUT.json MAC_TARBALL
#
# Writes OUT.json, the update manifest aturfile's in-app updater reads
# (crates/af-update/src/manifest.rs), and OUT.json.sig: base64 of the Ed25519
# signature over OUT.json's exact bytes.
#
#   VERSION      the workspace version (1.1.0)
#   NOTES_URL    release page (also the "download" link for copies that can't update themselves)
#   BASE_URL     where assets are downloaded from; the asset URL is BASE_URL/<file name>
#   MAC_TARBALL  aturfile-X.Y.Z-universal-macos.tar.gz (manifest key macos-universal-app)
#
# The signing key comes from UPDATE_SIGNING_KEY: base64 of the raw 32-byte Ed25519 seed (the
# GitHub `release` environment secret). Its public key must match the key the app embeds, so a
# wrong secret fails here instead of in the field. Needs OpenSSL 3 (Ed25519 with -rawin; set
# OPENSSL to its path where `openssl` is LibreSSL, as on macOS). Never prints the key.
set -euo pipefail
set +x

# The key in crates/af-update/src/manifest.rs.
EMBEDDED_PUBLIC_KEY="TRVgr/ZQFaOZl6Owb8HMAqa+qqx5mDGehe8241v1jHw="
OPENSSL="${OPENSSL:-openssl}"

[ $# -eq 5 ] || { sed -n '2,16p' "$0" >&2; exit 2; }
version="$1"; notes="$2"; base="${3%/}"; out="$4"; tarball="$5"

echo "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$' \
  || { echo "error: version '$version' is not semver" >&2; exit 2; }
[ -n "${UPDATE_SIGNING_KEY:-}" ] || { echo "error: UPDATE_SIGNING_KEY is not set" >&2; exit 1; }
[ -f "$tarball" ] || { echo "error: $tarball not found" >&2; exit 1; }

sha256() { if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1; }
json_str() { # no quotes, backslashes or control characters in our URLs and names
  case "$1" in *[\"\\]*) echo "error: unexpected character in '$1'" >&2; exit 2 ;; esac
  printf '"%s"' "$1"
}

mkdir -p "$(dirname "$out")"
cat > "$out" <<EOF
{
  "channel": "stable",
  "version": $(json_str "$version"),
  "date": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "notes_url": $(json_str "$notes"),
  "assets": {
    "macos-universal-app": {
      "url": $(json_str "$base/$(basename "$tarball")"),
      "sha256": "$(sha256 "$tarball")",
      "size": $(wc -c < "$tarball" | tr -d ' ')
    }
  }
}
EOF

# Sign. The key only ever lives in a private temp dir.
work="$(umask 077 && mktemp -d)"
trap 'rm -rf "$work"' EXIT
(
  umask 077
  # PKCS#8 DER for an Ed25519 private key = this fixed prefix + the 32-byte seed.
  printf '\x30\x2e\x02\x01\x00\x30\x05\x06\x03\x2b\x65\x70\x04\x22\x04\x20' > "$work/key.der"
  printf '%s' "$UPDATE_SIGNING_KEY" | "$OPENSSL" base64 -d -A > "$work/seed"
  [ "$(wc -c < "$work/seed" | tr -d ' ')" = 32 ] || { echo "error: UPDATE_SIGNING_KEY is not base64 of 32 bytes" >&2; exit 1; }
  cat "$work/seed" >> "$work/key.der"
  rm -f "$work/seed"
)
pub="$("$OPENSSL" pkey -inform DER -in "$work/key.der" -pubout -outform DER | tail -c 32 | "$OPENSSL" base64 -A)"
[ "$pub" = "$EMBEDDED_PUBLIC_KEY" ] \
  || { echo "error: UPDATE_SIGNING_KEY's public key is $pub, expected $EMBEDDED_PUBLIC_KEY" >&2; exit 1; }

"$OPENSSL" pkeyutl -sign -rawin -inkey "$work/key.der" -keyform DER -in "$out" -out "$work/sig"
[ "$(wc -c < "$work/sig" | tr -d ' ')" = 64 ] || { echo "error: signing failed" >&2; exit 1; }
"$OPENSSL" base64 -A -in "$work/sig" > "$out.sig"
echo >> "$out.sig"

# Check it the way the app will: the public key alone, over the file as written.
"$OPENSSL" pkey -inform DER -in "$work/key.der" -pubout -out "$work/pub.pem"
"$OPENSSL" pkeyutl -verify -pubin -inkey "$work/pub.pem" -rawin -in "$out" -sigfile "$work/sig" >/dev/null \
  || { echo "error: the signature doesn't verify" >&2; exit 1; }

echo "wrote $out and $out.sig ($version, key $pub)"
cat "$out"

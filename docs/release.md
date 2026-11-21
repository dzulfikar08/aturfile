# Releasing aturfile on the Mac App Store

## 1. What already exists (set up 2026-11-21)

- **Bundle ID** `com.atur.aturfile` (Apple team F6Q6UFUK69), registered with
  In-App Purchase. It can't be changed now.
- **App Store Connect app** "aturfile", Apple ID 6837575624, SKU `aturfile-mac`,
  English (U.S.).
- **In-App Purchase** `com.atur.aturfile.pro` (`af_store::PRO_PRODUCT`),
  Non-Consumable, "aturfile Pro", US$19.99, all territories, Family Sharing on,
  with a review screenshot and note. State: Ready to Submit; attach it to the
  first version you submit.
- **Signing**: the team's Apple Distribution and Mac Installer Distribution
  certificates (shared with Voltrus Tools, valid until October 2027) and an App
  Store Connect API key, stored as secrets of the `release` environment
  (deployments from `main` only).

## 2. Release from CI

Actions -> Release -> Run workflow, or `gh workflow run release.yml -f upload=true`.
`.github/workflows/release.yml` imports the certificates into a temporary
keychain (`scripts/ci-keychain.sh`), fetches or creates the provisioning profile
through the API (`scripts/asc-profile.py`, named `aturfile-ci ...`), builds the
universal store bundle, checks it for private API, signs it, packages the .pkg
and uploads it to App Store Connect (or only validates it, without `upload`).
The build number is the workflow run number.

| Secret (`release` environment) | What |
| --- | --- |
| `APPLE_DIST_P12_BASE64` / `APPLE_DIST_P12_PASSWORD` | Apple Distribution certificate and key |
| `MAC_INSTALLER_P12_BASE64` / `MAC_INSTALLER_P12_PASSWORD` | 3rd Party Mac Developer Installer certificate and key |
| `ASC_KEY_ID`, `ASC_ISSUER_ID`, `ASC_KEY_BASE64` | App Store Connect API key (base64 of the .p8) |

Still to do in App Store Connect before submitting: listing screenshots
(1280x800 or larger), description, support and privacy policy URLs, privacy
label "Data Not Collected" (no network at all), age rating 4+, and the Paid
Applications agreement, banking and tax (needed even to test the purchase in
TestFlight).

## 3. What Pro is

Free (and every source or GitHub build): browsing, tabs limited to one,
List/Columns/Icons, search, preview, file operations with undo, live
updates, themes, Clean Up's scan, report and staging.

Pro (App Store builds, `store` feature only): more than one tab, and Clean
Up's "Move to Trash" for the staged queue. Builds without the feature are
fully unlocked; the gate is `af_store::STORE_BUILD` plus the cached StoreKit
entitlement (`pro.json` in the container).

## 4. Build, sign, upload on a Mac by hand

With the two identities in the keychain:

```
scripts/build-mas.sh bundle dist-store
MAS_PROFILE=~/Downloads/aturfile_MAS.provisionprofile \
ASC_KEY_ID=XXXXXXXXXX ASC_ISSUER_ID=xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx \
  scripts/build-mas.sh package dist-store/aturfile.app dist-store [--upload]
```

`altool` reads the API key from `~/.appstoreconnect/private_keys/AuthKey_<ASC_KEY_ID>.p8`.

## 5. Before the first submission

- Private AppKit API: GPUI's macOS crate is vendored without it
  (`vendor/gpui-pre-macos/PATCHES.md`); CI fails if it comes back.
- Screenshots for the listing (1280×800 or larger), a support URL and a
  privacy policy URL.
- Test the purchase with a sandbox Apple Account (TestFlight or a
  development-signed build with a StoreKit configuration file).

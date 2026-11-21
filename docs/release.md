# Releasing aturfile on the Mac App Store

Nothing here is automated yet: no signing secrets exist. CI already builds
the store variant on every push (`store` job: universal arm64 + x86_64,
`store` feature, sandboxed, signed ad hoc) and uploads
`aturfile-mas-unsigned`. These are the steps left to you.

## 1. Decide the bundle ID (before registering anything)

The code uses the placeholder **`com.atur.aturfile`** (`packaging/Info.plist`,
`scripts/build-mas.sh`, the window's `app_id`). A bundle ID can't be changed
after the app exists in App Store Connect, and the Pro product ID is derived
from it. Pick the final one (for example `com.joyodigitama.aturfile`, like
Voltrus Tools' `com.joyodigitama.voltrus-tools`), then change it in:

- `packaging/Info.plist` (`CFBundleIdentifier`)
- `scripts/build-mas.sh` (`BUNDLE_ID`, or pass `BUNDLE_ID=...`)
- `crates/af-app/src/main.rs` (`app_id`)
- `crates/af-store/src/lib.rs` (`PRO_PRODUCT`, see below)

## 2. Apple Developer and App Store Connect

1. **Identifiers**: register an App ID for the bundle ID (Certificates,
   Identifiers & Profiles). Capabilities: In-App Purchase (on by default).
   No other capability is needed: the sandbox entitlements are only
   `app-sandbox`, `files.user-selected.read-write` and
   `files.bookmarks.app-scope` (`packaging/entitlements.mas.plist`).
2. **Certificates**: an "Apple Distribution" certificate and a
   "Mac Installer Distribution" certificate (shown in the keychain as
   "3rd Party Mac Developer Installer").
3. **Profile**: a Mac App Store provisioning profile for the App ID.
4. **App Store Connect → Apps → New App**: platform macOS, the bundle ID,
   name "aturfile", SKU (any), primary language. Category: Utilities.
   Privacy: aturfile collects no data (no network at all), so the privacy
   label is "Data Not Collected". Age rating: 4+.
5. **In-App Purchase**: create a **Non-Consumable** with product ID
   **`<bundle id>.pro`** (today `com.atur.aturfile.pro`, the value of
   `af_store::PRO_PRODUCT`), reference name "aturfile Pro", a price tier,
   display name and description, and a review screenshot of the Pro sheet
   (`aturfile --sheet pro`; CI's `store-screenshots` has one). Attach it to the
   first version you submit.
6. **Paid Applications agreement**, banking and tax must be complete before
   the In-App Purchase can be sold or even tested in TestFlight.

## 3. What Pro is

Free (and every source or GitHub build): browsing, tabs limited to one,
List/Columns/Icons, search, preview, file operations with undo, live
updates, themes, Clean Up's scan, report and staging.

Pro (App Store builds, `store` feature only): more than one tab, and Clean
Up's "Move to Trash" for the staged queue. Builds without the feature are
fully unlocked; the gate is `af_store::STORE_BUILD` plus the cached StoreKit
entitlement (`pro.json` in the container).

## 4. Build, sign, upload

On a Mac with the two identities in the keychain:

```
scripts/build-mas.sh bundle dist-store
MAS_PROFILE=~/Downloads/aturfile_MAS.provisionprofile \
ASC_KEY_ID=XXXXXXXXXX ASC_ISSUER_ID=xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx \
  scripts/build-mas.sh package dist-store/aturfile.app dist-store [--upload]
```

Without `--upload`, `package` validates the .pkg with App Store Connect when
`ASC_KEY_ID` is set. `BUILD_NUMBER` sets `CFBundleVersion` (default: Unix
time). `altool` reads the API key from
`~/.appstoreconnect/private_keys/AuthKey_<ASC_KEY_ID>.p8`.

## 5. Secrets for signing in CI (later)

To add a `release` job like Voltrus Tools' `release.yml`, create these
repository secrets:

| Secret | What |
| --- | --- |
| `MAS_CERT_P12` | base64 of a .p12 holding the Apple Distribution certificate and key |
| `MAS_INSTALLER_P12` | base64 of a .p12 holding the Mac Installer Distribution certificate and key |
| `MAS_P12_PASSWORD` | the password of those .p12 files |
| `MAS_PROFILE_B64` | base64 of the Mac App Store provisioning profile |
| `ASC_KEY_ID` | App Store Connect API key ID (role: App Manager) |
| `ASC_ISSUER_ID` | the API key's issuer ID |
| `ASC_KEY_P8` | the contents of `AuthKey_<id>.p8` |

The job imports the .p12 files into a temporary keychain, writes the
profile and the .p8, then runs `scripts/build-mas.sh bundle` and
`package --upload`.

## 6. Before the first submission

- **Private API use in GPUI.** `gpui-pre-macos` 0.3.12 (GPUI's macOS
  platform, through gpui-kit) calls private AppKit selectors:
  `NSCursor _windowResizeNorthWestSouthEastCursor` and
  `_windowResizeNorthEastSouthWestCursor` (diagonal resize cursors, which
  aturfile never shows), `NSWindow _zoomFill:` (title bar double-click set
  to "Fill"), and overrides `_opaqueRectForWindowMoveWhenInTitlebar` and
  `_windowRestorationOptions`. App Review's scanner can reject apps for
  private selectors. Before submitting, vendor `gpui-pre-macos` with these
  removed (a `[patch.crates-io]`, as the VoltrusID fork did for the blur
  API) or confirm with a TestFlight upload that the binary passes. No
  `CGS*` calls, `dlopen`/`dlsym`, private frameworks or network clients are
  in the macOS dependency tree.
- Screenshots for the listing (1280×800 or larger), a support URL and a
  privacy policy URL.
- Test the purchase with a sandbox Apple Account (TestFlight or a
  development-signed build with a StoreKit configuration file).

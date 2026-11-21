# Notices

aturfile is licensed under the GNU General Public License v3.0 or later
(see LICENSE). It includes or is derived from:

- **Kantor** (same author). The design tokens, metrics, small components,
  font selection and theme recipe in `crates/af-ui` come from Kantor's
  `kantor_ui` and `kantor_theme` crates, relicensed here under the GPL by
  their author.
- **iTerm2-Color-Schemes** (https://github.com/mbadolato/iTerm2-Color-Schemes),
  MIT. The bundled theme palettes in `crates/af-ui/assets/themes.tsv`.
- **GPUI** (https://github.com/zed-industries/zed), Apache-2.0, published as
  the `gpui-pre-*` crates. Licence: `packaging/licenses/gpui-lic.txt`.
- **GPUI Component / GPUI Kit** (https://github.com/longbridge/gpui-component),
  Apache-2.0. Licence: `packaging/licenses/gpui-component-lic.txt`.
- **Lucide** icons (https://lucide.dev), ISC. Licence:
  `packaging/licenses/lucide-lic.txt`.

App bundles carry these licence texts in `Contents/Resources/Licenses`.

## vendor/gpui-pre-macos

A copy of `gpui-pre-macos` 0.3.12 (Zed's `gpui_macos`, Apache-2.0, licence in
`vendor/gpui-pre-macos/LICENSE-APACHE`), changed to use only public AppKit API
for the Mac App Store. The changes are listed in `vendor/gpui-pre-macos/PATCHES.md`.

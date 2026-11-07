# Phase 3a: Orthros on Android

See [notes-and-map.md](notes-and-map.md). Phase 3b (share sheet) follows
once this runs on a phone. Delete this file when 3a has shipped.

## Decisions

- **Built in CI only** (`.github/workflows/android.yml`): GitHub's Ubuntu
  runners have the Android SDK and NDK. `gen/android` is generated in CI
  (`tauri android init --ci`), not committed, until we need custom Kotlin
  (3b).
- **arm64 only** (`aarch64-linux-android`): every current phone.
- **Signing:** build the unsigned release APK, then `zipalign` +
  `apksigner` with a keystore from repository secrets
  (`ANDROID_KEYSTORE` base64, `ANDROID_KEYSTORE_PASSWORD`,
  `ANDROID_KEY_ALIAS`). Without secrets a throwaway key is made, so the
  artifact still installs (but cannot update a release install).
- **Delivery:** every run uploads the APK as an artifact; a `v*` tag also
  attaches `Orthros_<version>_arm64.apk` to the draft release.
- **Sync on the phone:** the desktop shows a QR code (server + token); the
  phone scans it with the official barcode-scanner plugin.
- **Phone layout:** bottom tab bar Today | Notes | Map | Search; Days is
  reached from Search (an empty search lists the days). An editing bar above
  the keyboard replaces Tab, Shift-Tab, Ctrl-Enter and Alt-Up/Down.
- **Files on the phone** live in the app's private data folder.

## 1. Rust (src-tauri)

- Move `#[cfg_attr(mobile, tauri::mobile_entry_point)]` onto `run()`
  (it sits on `prepare_linux` today, so Android would have no entry point).
- Desktop only (`#[cfg(desktop)]`): tray, capture window and command
  behaviour, `--connect`, single instance, global shortcut, hide-on-close,
  window size/title builder calls.
- Store root: desktop as now; mobile `app.path().app_data_dir()`. The store
  is created in `setup` on every platform and managed there.
- Sync config: `sync::config_path()` keeps the desktop location; on mobile
  it is `<data dir>/sync.json`, set once at setup (a `OnceLock`).
- Commands: `sync_info() -> Option<{ server, token }>` (for the QR code) and
  `connect_sync(server, token)` (writes the config through
  `sync::connect`, then `Sync::now()`).
- `tauri-plugin-barcode-scanner = "2"` for Android/iOS only, initialised
  under `#[cfg(mobile)]`; `capabilities/mobile.json` with
  `"platforms": ["android", "iOS"]` granting `barcode-scanner:default`;
  `capabilities/default.json` gets `"platforms": ["linux", "macOS", "windows"]`
  only if needed for the window labels.

## 2. Frontend

- `@tauri-apps/plugin-barcode-scanner` 2.5.3 and `qrcode-generator` 2.0.4.
- `api.ts`: `syncInfo()`, `connectSync(server, token)`, `isMobile`
  (user agent contains "Android", or `?mobile` for browser work).
- Sync popover: clicking the dot opens a small panel: status, "Sync now",
  and on desktop "Connect a phone" (QR of
  `{"orthros":1,"server":"...","token":"..."}`), on the phone "Scan to
  connect" (or "Connected to <server>").
- Phone layout (`body.is-mobile` plus narrow-width media query): bottom tab
  bar with safe-area padding, larger touch targets, pin buttons always
  visible, the map fills the screen above the tab bar.
- Editing bar (phone, while an editor has focus): outdent, indent, task
  toggle, move up, move down, using the existing commands from
  `src/editor/keys.ts` / `blocks.ts`.
- Search with an empty query lists the days on the phone.

## 3. CI

- `android.yml`: `workflow_dispatch` and `v*` tags. Java 17, Rust with the
  Android target, `npm ci`, `tauri android init --ci`,
  `tauri android build --apk --target aarch64`, sign, upload.

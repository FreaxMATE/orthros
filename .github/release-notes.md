A small update for the Android app.

## Fixed

- **Android: connecting to sync.** The sync control was hidden behind the phone's status bar. It now sits at the right of the tab bar and says **Connect** until the phone is connected. Tap it, then **Scan to connect**.
- **Android: theme switch.** It was hidden the same way and is now in that panel.

## Updating

- **Android:** install the new APK. If Android refuses to install it over your current one, uninstall the old app first. Notes that exist only on the phone are lost when you uninstall.
- **Desktop and sync server:** no update needed if you are on 0.5.0.

New in 0.5: notes, a map of your places, place cards and the Android app. See the [0.5.0 notes](https://github.com/FreaxMATE/orthros/releases/tag/v0.5.0). **Coming from 0.4 or earlier with sync:** update the server and all your devices together.

## Install the app

| | Download |
|---|---|
| **macOS**, Apple silicon | `Orthros_*_aarch64.dmg` |
| **macOS**, Intel | `Orthros_*_x64.dmg` |
| **Linux**, any distro | `Orthros_*_amd64.AppImage` or `Orthros_*_x86_64.flatpak` |
| **Debian, Ubuntu** | `Orthros_*_amd64.deb` |
| **Fedora, openSUSE** | `Orthros-*.x86_64.rpm` |
| **Nix** | `nix run github:FreaxMATE/orthros` |
| **Android**, arm64 | `Orthros_*_arm64.apk` |

macOS: the app isn't signed. After moving it to Applications, run once:
`xattr -dr com.apple.quarantine /Applications/Orthros.app`

## Sync server (optional)

```sh
docker run -d --name orthros -p 127.0.0.1:8787:8787 -v orthros:/data ghcr.io/freaxmate/orthros-server
```

or `curl -fsSL https://freaxmate.github.io/orthros/install-server.sh | sh` on a Linux server, or the `orthros-server-*` binaries below. See the [README](https://github.com/FreaxMATE/orthros#sync-your-devices).

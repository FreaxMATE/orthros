A small update for everyone who keeps notes from ChatGPT.

## Fixed

- **Math copied from ChatGPT now renders.** ChatGPT writes formulas as `\( ... \)` and `\[ ... \]`. Pasting converts them to `$...$` and `$$...$$`, so they are typeset right away and your files keep one style. Code blocks and inline code are left exactly as they are.
- **Formulas already saved in that form render too**, so earlier pastes look right without editing them.

## Updating

- **macOS:** quit Orthros from its menu bar icon, download the new `.dmg` and replace the app in Applications. Then run the `xattr` command below again.
- **Linux:** quit Orthros from its top bar icon (closing the window only hides it), then install the new package below.
- **Nix:** run `nix flake update orthros`, rebuild, then quit and reopen Orthros.
- **Sync server:** no update needed.

New in 0.4: tables, highlighted code blocks, blocks and notes attached to tasks, and faster task entry. See the [0.4.0 notes](https://github.com/FreaxMATE/orthros/releases/tag/v0.4.0).

## Install the app

| | Download |
|---|---|
| **macOS**, Apple silicon | `Orthros_*_aarch64.dmg` |
| **macOS**, Intel | `Orthros_*_x64.dmg` |
| **Linux**, any distro | `Orthros_*_amd64.AppImage` or `Orthros_*_x86_64.flatpak` |
| **Debian, Ubuntu** | `Orthros_*_amd64.deb` |
| **Fedora, openSUSE** | `Orthros-*.x86_64.rpm` |
| **Nix** | `nix run github:FreaxMATE/orthros` |

macOS: the app isn't signed. After moving it to Applications, run once:
`xattr -dr com.apple.quarantine /Applications/Orthros.app`

## Sync server (optional)

```sh
docker run -d --name orthros -p 127.0.0.1:8787:8787 -v orthros:/data ghcr.io/freaxmate/orthros-server
```

or `curl -fsSL https://freaxmate.github.io/orthros/install-server.sh | sh` on a Linux server, or the `orthros-server-*` binaries below. See the [README](https://github.com/FreaxMATE/orthros#sync-your-devices).

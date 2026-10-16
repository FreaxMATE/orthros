You can now move tasks around, and the sync light finally settles.

## What's new

### Move items

Hover a task and a small grip appears next to its checkbox. Drag it to reorder the day, drop it into tomorrow or any other day, or onto the **Later** tab to park it. Sub-items come along and are indented to fit their new place. Press `Esc` to cancel.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/FreaxMATE/orthros/v0.2.0/site/release/0.2.0-move-dark.png">
  <img alt="Dragging a task: a grip next to the checkbox and a line showing where it will land." src="https://raw.githubusercontent.com/FreaxMATE/orthros/v0.2.0/site/release/0.2.0-move-light.png">
</picture>

Prefer the keyboard? `Alt+Up` and `Alt+Down` move the task under the cursor along with its sub-items. Go past the first or last task and it moves into the day above or below.

### Room to breathe

The text column now grows with the window, and so do the margins. On a large monitor it stops at a comfortable reading width.

## Fixed

- **The sync light no longer pulses forever.** Orthros mistook its own file reads for changes, so every sync started the next one. Now it turns solid green once everything is sent.
- **The cursor no longer jumps back while you type.** The same loop could briefly show a slightly older copy of what you had just written, which moved the cursor a few words back.

## Updating

- **macOS:** quit Orthros from its menu bar icon, download the new `.dmg` and replace the app in Applications. Then run the `xattr` command below again.
- **Linux:** quit Orthros from its top bar icon (closing the window only hides it), then install the new package below.
- **Nix:** run `nix flake update orthros`, rebuild, then quit and reopen Orthros.
- **Sync server:** no update needed. Version 0.1.0 servers work with this release.

Your notes don't change: they stay plain markdown files.

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

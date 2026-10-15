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

or `curl -fsSL https://freaxmate.github.io/orthros/install-server.sh | sh` on a Linux server, or the `orthros-server-*` binaries below. See the [README](https://github.com/FreaxMATE/orthros#sync).

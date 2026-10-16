Write math the way you'd write it in your thesis.

## What's new

### LaTeX math

Put a formula between dollar signs and Orthros typesets it, right inside your tasks and notes.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/FreaxMATE/orthros/v0.3.0/site/release/0.3.0-math-dark.png">
  <img alt="A day in Orthros with typeset math: symbols inside tasks and a centred equation between them." src="https://raw.githubusercontent.com/FreaxMATE/orthros/v0.3.0/site/release/0.3.0-math-light.png">
</picture>

- `$\sigma^2 < 1$` sets math inside a sentence.
- `$$...$$` sets a centred equation of its own. It can span several lines, so `aligned`, matrices and `cases` work too.
- **Click a formula to edit it.** Its source shows while the cursor is on it and renders again when you move on.
- **Copying copies the LaTeX**, ready to paste into a `.tex` file.
- Dollar amounts like "5$" or "$5 and $10" stay text, and so does anything inside `code`.
- A formula with a mistake shows its source in red instead of breaking the line.

Math is drawn with [KaTeX](https://katex.org), fast enough to keep up while you type. Its fonts come with the app, so it works offline. Your files stay plain markdown, the same `$...$` that pandoc, Obsidian and GitHub understand.

## Updating

- **macOS:** quit Orthros from its menu bar icon, download the new `.dmg` and replace the app in Applications. Then run the `xattr` command below again.
- **Linux:** quit Orthros from its top bar icon (closing the window only hides it), then install the new package below.
- **Nix:** run `nix flake update orthros`, rebuild, then quit and reopen Orthros.
- **Sync server:** no update needed. Servers from 0.1.0 on work with this release.

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

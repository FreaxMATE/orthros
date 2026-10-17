Tables and code blocks, right under the task they belong to.

## What's new

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/FreaxMATE/orthros/v0.4.0/site/release/0.4.0-blocks-dark.png">
  <img alt="A day in Orthros: a table of results and a highlighted Python code block, each under its own task." src="https://raw.githubusercontent.com/FreaxMATE/orthros/v0.4.0/site/release/0.4.0-blocks-light.png">
</picture>

### Tables

Markdown tables show as a clean grid. Click a cell to edit, and the source appears:

- `Tab` and `Shift+Tab` jump between cells, and `Tab` after the last cell adds a row.
- `Enter` adds a row below.
- When you leave, the columns line up by themselves, keeping their alignment.
- Bold, `code`, links and `$math$` work inside cells.

To start a table, type `| Name | Value |` and press `Enter`.

### Code blocks

Fenced code blocks get a soft box, the language name and a copy button.

- **Highlighting** covers about 140 languages, from Python, Rust, C and Bash to LaTeX, SQL, YAML and Nix. Each loads only when you use it.
- **Starting a block:** type ` ```python ` and press `Enter`. The closing fence is added for you.
- **Inside a block**, the keys work like in a code editor: `Enter` keeps the indentation, and `Tab` indents.

### Attach things to a task

A code block, a table or a note can belong to a task, and it moves along when you drag the task or use `Alt+Up`/`Alt+Down`:

- type ` ``` ` or `|` on a new empty item to start a block under the task above
- press `Shift+Enter` in a task for a line without a checkbox

### Tasks with less typing

In a day, `Enter` at the end of any line now starts a new task, also after a heading, a note or plain text. For plain text instead, press `Shift+Enter`, or `Backspace` once on the new task. Later keeps its bullets.

## Fixed

- **Linux:** Orthros no longer fails to start on some graphics setups (often NVIDIA, or in the Flatpak) with "Could not create default EGL display: EGL_BAD_PARAMETER".
- **Linux:** the harmless "libayatana-appindicator is deprecated" warning no longer shows up on every start.

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

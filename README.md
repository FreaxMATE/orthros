<p align="center">
  <img src="site/icon.svg" width="72" height="72" alt="">
</p>

<h1 align="center">Orthros</h1>

<p align="center">A quiet daily todo journal for macOS and Linux.</p>

<p align="center">
  <a href="https://github.com/FreaxMATE/orthros/actions/workflows/ci.yml"><img src="https://github.com/FreaxMATE/orthros/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/FreaxMATE/orthros/releases/latest"><img src="https://img.shields.io/github/v/release/FreaxMATE/orthros?label=release&color=b4441a" alt="Latest release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-AGPL--3.0-6f6a62" alt="License: AGPL-3.0"></a>
</p>

<p align="center">
  <a href="https://freaxmate.github.io/orthros/">Website</a> ·
  <a href="https://github.com/FreaxMATE/orthros/releases/latest">Download</a>
</p>

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="site/screenshot-dark.png">
  <img alt="Orthros showing tomorrow's and today's lists with checkboxes, nested tasks and a short note." src="site/screenshot-light.png">
</picture>

Write down the few things that matter today. Park everything else in **Later**.
Orthros keeps your days as plain markdown files and syncs them with a server
you run yourself.

## Features

- **One column of days.** Tomorrow, today and the days before, so you can plan the evening before.
- **Later.** A separate list for ideas and someday tasks, out of today's way.
- **Markdown that stays out of the way.** Checkboxes, nesting, headings, bold, code and links; the syntax only shows on the line you're editing.
- **Quick capture.** One shortcut from anywhere opens a small popup: type, Enter, done.
- **Plain files.** Every day is a `.md` file you can grep, back up or open in any editor.
- **Self-hosted sync.** Offline-first. Edits from several devices merge line by line, and nothing is overwritten.

## Install

**macOS and Linux:** download the `.dmg`, `.AppImage` or `.deb` from the
[latest release](https://github.com/FreaxMATE/orthros/releases/latest).
The macOS app isn't signed; after copying it to Applications, run once:

```sh
xattr -dr com.apple.quarantine /Applications/Orthros.app
```

**Nix:** `nix run github:FreaxMATE/orthros`, or add
`inputs.orthros.packages.${system}.orthros` to your packages.

**Quick capture:** `Ctrl-Option-Space` on macOS. On GNOME, bind a custom
shortcut to `orthros --capture`. Closing the window keeps Orthros in the top
bar (GNOME needs the AppIndicator extension).

## Sync

Run the server, then point each device at it.

```nix
# NixOS, with this flake as an input
imports = [ inputs.orthros.nixosModules.server ];
services.orthros-server = {
  enable = true;                                  # 127.0.0.1:8787
  tokenFile = "/var/lib/secrets/orthros-token";   # head -c 32 /dev/urandom | base64
};
```

Elsewhere: `orthros-server --token-file FILE [--data DIR] [--listen ADDR:PORT]`.
Put HTTPS in front of it, for example `tailscale serve --bg --https=8444 http://127.0.0.1:8787`.

On each device, create `sync.json` in `~/.config/orthros/` (macOS:
`~/Library/Application Support/orthros/`):

```json
{ "server": "https://your-server:8444", "token_file": "~/.config/orthros/token" }
```

## Keys

| Key | Action |
|---|---|
| `Enter` / `Tab` / `Shift-Tab` | new item, indent, outdent |
| `Ctrl-Enter` | tick or untick; makes a line a task |
| `[] ` at the start of a line | becomes a task |
| `Ctrl-1` … `Ctrl-4` | Today, Later, Days, Search |
| `Ctrl-K` | search |

On macOS, `Cmd` instead of `Ctrl`.

## Development

```sh
nix develop            # or: Rust, Node and the Tauri prerequisites
npm install
npm run tauri dev
cargo test --workspace
```

Built with [Tauri](https://tauri.app), Rust and [CodeMirror](https://codemirror.net).

## License

[AGPL-3.0](LICENSE)

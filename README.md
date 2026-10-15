# orthros

*Orthros (ὄρθρος): the hour before dawn, when you plan the day.*

A quiet daily todo journal for macOS and Linux. One column of days
(tomorrow, today, then everything before), each a short list you write in the
morning or the evening before. Plus a **Later** list for the things you want
to come back to someday, so they stop circling in your head.

Your data is a folder of plain markdown files:

```
~/.local/share/orthros/            (macOS: ~/Library/Application Support/orthros)
├── days/2026-10-15.md
├── days/2026-10-16.md
└── later.md
```

Override the location with `ORTHROS_DIR=/some/folder`.

## Writing

Markdown is drawn as you type and only shows on the line you're editing.

| Key | Does |
|---|---|
| `Enter` | new item (a new checkbox after a task); on an empty item, end the list |
| `Tab` / `Shift-Tab` | indent / outdent |
| `Ctrl-Enter` (`Cmd-Enter`) | tick / untick; turns a line into a task |
| `Backspace` at the start of an item | outdent, then task → bullet → plain line |
| `[] ` at the start of a line | becomes a task |
| `↑` / `↓` past the first / last line | move to the neighbouring day |
| `Ctrl-1…4` (`Cmd`) | Today / Later / Days / Search |
| `Ctrl-K` (`Cmd-K`) | search |

Also: `## Heading`, `**bold**`, `_italic_`, `` `code` ``, `~~strike~~`,
`[links](https://…)` and bare URLs (click to open).

## Quick capture

A small popup for a thought you don't want to lose: type it, `Enter` saves it
to **Later** (or `Tab` first: a task for today), `Esc` closes.

- **Linux (GNOME):** bind a custom shortcut to `orthros --capture`
  (Settings, Keyboard, Custom Shortcuts; or via dconf/Home Manager). GNOME on
  Wayland doesn't let apps register global shortcuts themselves.
- **macOS:** `Ctrl-Option-Space` while Orthros is running.

Closing the main window keeps Orthros running in the top bar / menu bar
(GNOME needs the *AppIndicator and KStatusNotifierItem Support* extension).
Quit from its menu there.

## Sync

Every device keeps the whole space locally and works offline. A small server
(`orthros-server`) is the hub. Edits to different lines merge; if the same
line was changed on two devices, both versions are kept, one under the
other. The dot in the corner shows the state (words only when something is
wrong); click it to sync now.

**Server** (NixOS):

```nix
# flake inputs: orthros.url = "github:FreaxMATE/orthros";
imports = [ inputs.orthros.nixosModules.server ];
services.orthros-server = {
  enable = true;                          # listens on 127.0.0.1:8787
  tokenFile = "/var/lib/secrets/orthros-token";
};
```

Generate a token with `head -c 32 /dev/urandom | base64`, and put HTTPS in
front, for example `tailscale serve --bg --https=8444 http://127.0.0.1:8787`.
Without NixOS: `orthros-server --token-file FILE [--data DIR] [--listen ADDR:PORT]`.

**Each device:** `~/.config/orthros/sync.json`
(macOS: `~/Library/Application Support/orthros/sync.json`):

```json
{ "server": "https://your-server.example.ts.net:8444", "token_file": "~/.config/orthros/token" }
```

with the token in that file (`chmod 600`), or inline as `"token": "..."`.

## Install

Nix (Linux): `nix run github:FreaxMATE/orthros`, or add
`inputs.orthros.packages.${system}.orthros` to your packages.

macOS: see Develop; `npm run tauri build` gives `Orthros.app`.

## Develop

Linux (Nix): `direnv allow` once, or `nix develop`, then

```sh
npm install
npm run tauri dev      # the app, with hot reload
npm run tauri build    # release build
```

The UI alone runs in a browser with `npm run dev`; open
`http://localhost:1420/?demo` for sample data (stored in localStorage).

macOS: install [rustup](https://rustup.rs) and Node, then the same `npm`
commands. `npm run tauri build` produces `Orthros.app` and a `.dmg` under
`src-tauri/target/release/bundle/`.

Tests (merging, the server, two devices syncing through it): `cargo test --workspace`.

## Stack

[Tauri 2](https://tauri.app) with a small Rust core (files, search, sync)
and a TypeScript front end built on [CodeMirror 6](https://codemirror.net).
`core/` is shared with `server/` (axum).

## License

AGPL-3.0-only. See [LICENSE](LICENSE).

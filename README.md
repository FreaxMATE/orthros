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

Rust tests: `cd src-tauri && cargo test`.

## Stack

[Tauri 2](https://tauri.app) with a small Rust core (files, search) and a
TypeScript front end built on [CodeMirror 6](https://codemirror.net).

## License

AGPL-3.0-only. See [LICENSE](LICENSE).

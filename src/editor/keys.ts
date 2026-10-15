// Outliner keys on top of plain markdown text.
//
//   Enter            continue the list (new unchecked task / bullet);
//                    on an empty item, end the list
//   Tab / Shift-Tab  indent / outdent the selected lines
//   Mod-Enter        tick or untick; turns a bullet or plain line into a task
//   Backspace        at the start of an item: outdent, then task → bullet →
//                    plain line, one step per press
//   "[] "            at the start of a line becomes a task

import { indentUnit } from "@codemirror/language";
import { insertNewlineContinueMarkup } from "@codemirror/lang-markdown";
import { EditorSelection, type ChangeSpec, type EditorState, type Line } from "@codemirror/state";
import { EditorView, keymap, type Command } from "@codemirror/view";
import { lineInfo, toggleTask } from "./livePreview";

const UNIT = "  ";

function selectedLines(state: EditorState): Line[] {
  const seen = new Map<number, Line>();
  for (const r of state.selection.ranges) {
    for (let n = state.doc.lineAt(r.from).number; n <= state.doc.lineAt(r.to).number; n++) {
      seen.set(n, state.doc.line(n));
    }
  }
  return [...seen.values()];
}

const indent: Command = (view) => {
  const changes = selectedLines(view.state).map((l) => ({ from: l.from, insert: UNIT }));
  view.dispatch(view.state.update({ changes, userEvent: "input.indent" }));
  return true;
};

const outdent: Command = (view) => {
  const changes: ChangeSpec[] = [];
  for (const l of selectedLines(view.state)) {
    const n = Math.min(/^ */.exec(l.text)![0].length, UNIT.length);
    if (n) changes.push({ from: l.from, to: l.from + n });
  }
  if (changes.length) view.dispatch(view.state.update({ changes, userEvent: "delete.dedent" }));
  return true;
};

const toggle: Command = (view) => {
  const changes: ChangeSpec[] = [];
  for (const l of selectedLines(view.state)) {
    const info = lineInfo(l.text);
    if (info.kind === "task") {
      changes.push(toggleTask(view.state, l.from + info.checkAt).changes);
    } else if (info.kind === "bullet") {
      changes.push({ from: l.from + info.contentStart, insert: "[ ] " });
    } else {
      const lead = l.text.length - l.text.trimStart().length;
      changes.push({ from: l.from + lead, insert: "- [ ] " });
    }
  }
  view.dispatch(view.state.update({ changes, userEvent: "input.toggle" }));
  return true;
};

const backspace: Command = (view) => {
  const sel = view.state.selection.main;
  if (!sel.empty || view.state.selection.ranges.length > 1) return false;
  const line = view.state.doc.lineAt(sel.head);
  const info = lineInfo(line.text);
  if (sel.head !== line.from + info.contentStart || info.contentStart === 0) return false;

  if (info.level > 0) return outdent(view);
  if (info.kind === "task") {
    // "- [ ] x" → "- x"
    view.dispatch({ changes: { from: line.from + info.checkAt - 1, to: line.from + info.contentStart }, userEvent: "delete" });
  } else {
    view.dispatch({ changes: { from: line.from, to: line.from + info.contentStart }, userEvent: "delete" });
  }
  return true;
};

/** "[] " or "- [] " typed at the start of a line becomes "- [ ] ". */
const taskShortcut = EditorView.inputHandler.of((view, from, to, text) => {
  if (text !== " " || from !== to) return false;
  const line = view.state.doc.lineAt(from);
  const before = line.text.slice(0, from - line.from);
  const m = /^(\s*)(?:- )?\[\]$/.exec(before);
  if (!m) return false;
  const insert = `${m[1]}- [ ] `;
  view.dispatch({
    changes: { from: line.from, to: from, insert },
    selection: EditorSelection.cursor(line.from + insert.length),
    userEvent: "input",
  });
  return true;
});

/** Arrow keys at the first/last line hand focus to the neighbouring day. */
export function boundaryKeys(onLeave: (dir: -1 | 1) => boolean) {
  const at = (dir: -1 | 1): Command => (view) => {
    const sel = view.state.selection.main;
    if (!sel.empty) return false;
    const line = view.state.doc.lineAt(sel.head).number;
    const edge = dir < 0 ? 1 : view.state.doc.lines;
    if (line !== edge) return false;
    // Let the cursor move within a wrapped line before leaving.
    const moved = view.moveVertically(sel, dir > 0);
    if (view.state.doc.lineAt(moved.head).number === line && moved.head !== sel.head) return false;
    return onLeave(dir);
  };
  return keymap.of([
    { key: "ArrowUp", run: at(-1) },
    { key: "ArrowDown", run: at(1) },
  ]);
}

export const outlinerKeys = [
  indentUnit.of(UNIT),
  taskShortcut,
  keymap.of([
    { key: "Enter", run: insertNewlineContinueMarkup },
    { key: "Tab", run: indent, shift: outdent },
    { key: "Mod-Enter", run: toggle },
    { key: "Backspace", run: backspace },
  ]),
];

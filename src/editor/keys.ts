// Outliner keys on top of plain markdown text.
//
//   Enter            continue the list (new unchecked task / bullet);
//                    on an empty item, end the list. In days, Enter at the
//                    end of any other line starts a new task too (see
//                    newItemKeys)
//   Tab / Shift-Tab  indent / outdent the selected lines
//   Mod-Enter        tick or untick; turns a bullet or plain line into a task
//   Backspace        at the start of an item: outdent, then task → bullet →
//                    plain line, one step per press
//   "[] "            at the start of a line becomes a task
//   Shift-Enter      a line under the current item, without a checkbox (a
//                    note, a table, code), which belongs to the item; after
//                    a plain line, just a new plain line
//   "```" or "|"     typed on a fresh empty item: that line becomes a code
//                    block or table under the item above

import { indentUnit } from "@codemirror/language";
import { insertNewlineContinueMarkup } from "@codemirror/lang-markdown";
import { EditorSelection, Prec, type ChangeSpec, type EditorState, type Line } from "@codemirror/state";
import { EditorView, keymap, type Command } from "@codemirror/view";
import { fenceAt } from "./code";
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
  // A fresh, empty task becomes a plain line in one step.
  if (info.kind === "task" && line.text.length === info.contentStart) {
    view.dispatch({ changes: { from: line.from, to: line.to }, userEvent: "delete" });
    return true;
  }
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

const EMPTY_ITEM = /^(\s*)[-*+] (?:\[[ xX]\] )?$/;

/** "```" or "|" on an empty item: the start of a block under the item above
 *  (or a free-standing one, without an item above). */
const blockShortcut = EditorView.inputHandler.of((view, from, to, text) => {
  if (from !== to || (text !== "`" && text !== "|")) return false;
  const line = view.state.doc.lineAt(from);
  if (from !== line.to) return false;
  const typed = line.text + text;
  const m = /^(\s*)[-*+] (?:\[[ xX]\] )?(```|\|)$/.exec(typed);
  if (!m) return false;
  const above = line.number > 1 ? lineInfo(view.state.doc.line(line.number - 1).text) : null;
  const underItem = !!above && (above.kind === "task" || above.kind === "bullet" || above.level > 0);
  const insert = (underItem ? m[1] + UNIT : m[1]) + m[2];
  view.dispatch({
    changes: { from: line.from, to: line.to, insert },
    selection: EditorSelection.cursor(line.from + insert.length),
    userEvent: "input",
  });
  return true;
});

/** Shift-Enter: a new line indented under the current item, no checkbox. */
const continueItem: Command = (view) => {
  const sel = view.state.selection.main;
  const line = view.state.doc.lineAt(sel.head);
  const info = lineInfo(line.text);
  if (EMPTY_ITEM.test(line.text) && sel.head === line.to) return false;
  if (fenceAt(view.state, sel.head)) return false;
  const pad = /^\s*/.exec(line.text)![0] + (info.kind === "task" || info.kind === "bullet" ? UNIT : "");
  view.dispatch({
    changes: { from: sel.from, to: sel.to, insert: "\n" + pad },
    selection: EditorSelection.cursor(sel.from + 1 + pad.length),
    scrollIntoView: true,
    userEvent: "input",
  });
  return true;
};

/** Days: Enter at the end of a heading, a plain line or an empty line starts
 *  a new item (`prefix`, "- [ ] ") instead of a plain line. After a note under
 *  an item, the new item is that item's sibling. */
export function newItemKeys(prefix: string) {
  const run: Command = (view) => {
    const { state } = view;
    const sel = state.selection.main;
    if (!sel.empty || state.selection.ranges.length > 1) return false;
    const line = state.doc.lineAt(sel.head);
    if (sel.head !== line.to) return false;
    const info = lineInfo(line.text);
    if (info.kind === "task" || info.kind === "bullet") return false;
    // Inside a code block (but its closing fence is fine).
    const f = fenceAt(state, sel.head);
    if (f && !(f.closed && line.number === f.last)) return false;
    const lead = /^\s*/.exec(line.text)![0].length;
    const indent = UNIT.repeat(Math.max(0, Math.floor(lead / 2) - (line.text.trim() ? 1 : 0)));
    const insert = "\n" + indent + prefix;
    view.dispatch({
      changes: { from: sel.head, insert },
      selection: EditorSelection.cursor(sel.head + insert.length),
      scrollIntoView: true,
      userEvent: "input",
    });
    return true;
  };
  return Prec.highest(keymap.of([{ key: "Enter", run }]));
}

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
  blockShortcut,
  keymap.of([
    { key: "Enter", run: insertNewlineContinueMarkup },
    { key: "Shift-Enter", run: continueItem },
    { key: "Tab", run: indent, shift: outdent },
    { key: "Mod-Enter", run: toggle },
    { key: "Backspace", run: backspace },
  ]),
];

// Markdown tables, drawn as a grid until the cursor touches them; then the
// source shows for editing, and the pipes are lined up again on leaving.
//
// While editing: Tab / Shift-Tab jump between cells (Tab past the last cell
// adds a row), Enter adds a row below. Enter on a line like "| a | b |" that
// isn't a table yet adds the "|---|---|" line under it.

import { syntaxTree } from "@codemirror/language";
import { EditorSelection, Prec, StateEffect, StateField, type EditorState, type Range, type Transaction } from "@codemirror/state";
import { Decoration, EditorView, WidgetType, keymap, type Command, type DecorationSet } from "@codemirror/view";
import katex from "katex";

/** "none" is left aligned without saying so ("---" rather than ":--"). */
type Align = "none" | "left" | "center" | "right";

export interface Table {
  from: number;
  to: number;
  first: number;
  last: number;
}

const DELIM = /^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?\s*$/;
const ROW = /^\s*\|.*\|\s*$/;

/** Split a table row into its cells (trimmed); "|" inside `code` or escaped
 *  as "\|" doesn't split. */
export function splitRow(text: string): { cells: string[]; starts: number[] } {
  const cells: string[] = [];
  const starts: number[] = [];
  let i = text.indexOf("|");
  if (i < 0) return { cells: [text.trim()], starts: [0] };
  let code = false;
  let cur = i + 1;
  for (let k = i + 1; k <= text.length; k++) {
    const c = text[k];
    if (c === "\\") {
      k++;
      continue;
    }
    if (c === "`") code = !code;
    if ((c === "|" && !code) || k === text.length) {
      const raw = text.slice(cur, k);
      if (k < text.length || raw.trim()) {
        cells.push(raw.trim());
        // An empty cell starts one space in, where typing looks natural.
        starts.push(cur + (raw.trim() ? raw.length - raw.trimStart().length : Math.min(1, raw.length)));
      }
      cur = k + 1;
    }
  }
  return { cells, starts };
}

function aligns(delim: string): Align[] {
  return splitRow(delim).cells.map((c) =>
    c.startsWith(":") && c.endsWith(":") ? "center" : c.endsWith(":") ? "right" : c.startsWith(":") ? "left" : "none",
  );
}

export function tables(state: EditorState): Table[] {
  const out: Table[] = [];
  syntaxTree(state).iterate({
    enter: (n) => {
      if (n.name !== "Table") return;
      const first = state.doc.lineAt(n.from);
      const last = state.doc.lineAt(n.to);
      out.push({ from: first.from, to: last.to, first: first.number, last: last.number });
      return false;
    },
  });
  return out;
}

/** The table's source with every column padded to the same width. */
export function formatTable(state: EditorState, t: Table): string {
  const indent = /^\s*/.exec(state.doc.line(t.first).text)![0];
  const rows: string[][] = [];
  let align: Align[] = [];
  for (let n = t.first; n <= t.last; n++) {
    const text = state.doc.line(n).text;
    if (n === t.first + 1 && DELIM.test(text)) align = aligns(text);
    else rows.push(splitRow(text).cells);
  }
  const cols = Math.max(align.length, ...rows.map((r) => r.length));
  const width = Array.from({ length: cols }, (_, c) => Math.max(3, ...rows.map((r) => (r[c] ?? "").length)));
  const pad = (s: string, c: number) => {
    const w = width[c] - s.length;
    if (align[c] === "right") return " ".repeat(w) + s;
    if (align[c] === "center") return " ".repeat(Math.floor(w / 2)) + s + " ".repeat(Math.ceil(w / 2));
    return s + " ".repeat(w);
  };
  const line = (cells: string[]) => `${indent}| ${cells.join(" | ")} |`;
  const delim = width.map((w, c) => {
    const a = align[c] ?? "none";
    const dashes = "-".repeat(w - (a === "center" ? 2 : a === "none" ? 0 : 1));
    return a === "center" ? `:${dashes}:` : a === "right" ? `${dashes}:` : a === "left" ? `:${dashes}` : dashes;
  });
  const [head, ...body] = rows.map((r) => Array.from({ length: cols }, (_, c) => pad(r[c] ?? "", c)));
  return [line(head), line(delim), ...body.map(line)].join("\n");
}

// ---------------------------------------------------------------- drawing

const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

/** A cell's inline markdown as HTML: code, math, bold, italic, strike, links. */
function inline(src: string): string {
  const keep: string[] = [];
  const hold = (html: string) => `\u0000${keep.push(html) - 1}\u0000`;
  let s = src
    .replace(/\\\|/g, "|")
    .replace(/`([^`]+)`/g, (_, c) => hold(`<code class="cm-code">${esc(c)}</code>`))
    .replace(/\$([^\s$](?:[^$]*[^\s$])?)\$(?!\d)/g, (_, m) =>
      hold(`<span class="cm-math">${katex.renderToString(m, { throwOnError: false })}</span>`),
    );
  s = esc(s)
    .replace(/\[([^\]]*)\]\(([^)\s]*)\)/g, (_, t, u) => `<span class="cm-link" data-href="${u}">${t}</span>`)
    .replace(/(\*\*|__)(.+?)\1/g, '<strong class="cm-strong">$2</strong>')
    .replace(/(\*|_)(.+?)\1/g, '<em class="cm-em">$2</em>')
    .replace(/~~(.+?)~~/g, '<s class="cm-strike">$1</s>');
  return s.replace(/\u0000(\d+)\u0000/g, (_, i) => keep[Number(i)]);
}

class TableWidget extends WidgetType {
  /** `level`: how deep the table sits in a list (under which item). */
  constructor(readonly source: string, readonly level: number) {
    super();
  }
  eq(o: TableWidget) {
    return o.source === this.source && o.level === this.level;
  }
  toDOM() {
    const lines = this.source.split("\n");
    const align = aligns(lines[1] ?? "");
    const wrap = document.createElement("div");
    wrap.className = "cm-table-wrap";
    wrap.style.paddingLeft = `calc(${this.level} * var(--indent))`;
    const table = document.createElement("table");
    table.className = "cm-table";
    const rows = [lines[0], ...lines.slice(2)];
    rows.forEach((text, r) => {
      const tr = document.createElement("tr");
      const { cells } = splitRow(text);
      const cols = Math.max(align.length, cells.length);
      for (let c = 0; c < cols; c++) {
        const td = document.createElement(r === 0 ? "th" : "td");
        td.innerHTML = inline(cells[c] ?? "");
        td.style.textAlign = align[c] === "center" || align[c] === "right" ? align[c] : "left";
        td.dataset.row = String(r === 0 ? 0 : r + 1);
        td.dataset.col = String(c);
        tr.append(td);
      }
      (r === 0 ? table.createTHead() : table.tBodies[0] ?? table.createTBody()).append(tr);
    });
    wrap.append(table);
    return wrap;
  }
  ignoreEvent() {
    return false;
  }
}

// ---------------------------------------------------------------- state

const setFocus = StateEffect.define<boolean>();

interface TableState {
  list: Table[];
  focused: boolean;
  deco: DecorationSet;
}

const touches = (state: EditorState, t: Table) => state.selection.ranges.some((r) => r.from <= t.to && r.to >= t.from);

function decorate(state: EditorState, list: Table[], focused: boolean): DecorationSet {
  const deco: Range<Decoration>[] = [];
  for (const t of list) {
    if (focused && touches(state, t)) {
      for (let n = t.first; n <= t.last; n++) deco.push(Decoration.line({ class: "cm-table-src" }).range(state.doc.line(n).from));
    } else {
      const level = Math.floor(/^ */.exec(state.doc.line(t.first).text)![0].length / 2);
      const widget = new TableWidget(state.sliceDoc(t.from, t.to).replace(/^\s+/gm, ""), level);
      deco.push(Decoration.replace({ widget, block: true }).range(t.from, t.to));
    }
  }
  return Decoration.set(deco, true);
}

export const tableField = StateField.define<TableState>({
  create(state) {
    const list = tables(state);
    return { list, focused: false, deco: decorate(state, list, false) };
  },
  update(v, tr: Transaction) {
    let { list, focused } = v;
    for (const e of tr.effects) if (e.is(setFocus)) focused = e.value;
    if (tr.docChanged || syntaxTree(tr.startState) !== syntaxTree(tr.state)) list = tables(tr.state);
    if (list === v.list && focused === v.focused && !tr.selection) return v;
    return { list, focused, deco: decorate(tr.state, list, focused) };
  },
  provide: (f) => EditorView.decorations.from(f, (v) => v.deco),
});

export function tableAt(state: EditorState, pos: number): Table | null {
  const f = state.field(tableField, false);
  return f?.list.find((t) => pos >= t.from && pos <= t.to) ?? null;
}

/** Line up the pipes of every table the cursor just left. */
const tidy = EditorView.updateListener.of((u) => {
  if (!u.selectionSet && !u.focusChanged) return;
  const before = u.startState.field(tableField, false);
  const after = u.state.field(tableField);
  if (!before?.focused) return;
  const left = before.list
    .filter((t) => touches(u.startState, t))
    .map((t) => ({ from: u.changes.mapPos(t.from), to: u.changes.mapPos(t.to, 1) }))
    .map((r) => after.list.find((t) => t.from === r.from && t.to === r.to))
    .filter((t): t is Table => !!t && !(after.focused && touches(u.state, t)));
  const changes = left
    .map((t) => ({ from: t.from, to: t.to, insert: formatTable(u.state, t) }))
    .filter((c) => c.insert !== u.state.sliceDoc(c.from, c.to));
  if (changes.length) {
    // Not from inside an update: right after it.
    queueMicrotask(() => {
      const mapped = changes.filter((c) => u.view.state.sliceDoc(c.from, c.to) === u.state.sliceDoc(c.from, c.to));
      if (mapped.length && u.view.state.doc === u.state.doc) u.view.dispatch({ changes: mapped, userEvent: "input.format" });
    });
  }
});

// ---------------------------------------------------------------- keys

/** Cell starts (document positions) of every row except the delimiter row. */
function cellStarts(state: EditorState, t: Table): number[][] {
  const out: number[][] = [];
  for (let n = t.first; n <= t.last; n++) {
    if (n === t.first + 1) continue;
    const line = state.doc.line(n);
    out.push(splitRow(line.text).starts.map((s) => line.from + s));
  }
  return out;
}

function emptyRow(state: EditorState, t: Table) {
  const head = state.doc.line(t.first).text;
  const indent = /^\s*/.exec(head)![0];
  const cols = splitRow(head).cells.length;
  return `${indent}|${"   |".repeat(cols)}`;
}

const nextCell =
  (dir: 1 | -1): Command =>
  (view) => {
    const { state } = view;
    const { head, from } = state.selection.main;
    const t = tableAt(state, head);
    if (!t) return false;
    const flat = cellStarts(state, t).flat();
    let i: number;
    if (dir > 0) {
      i = flat.findIndex((p) => p > head);
    } else {
      let here = -1;
      flat.forEach((p, k) => {
        if (p <= from) here = k;
      });
      i = here - 1;
    }
    if (i >= 0) {
      const line = state.doc.lineAt(flat[i]);
      const { cells, starts } = splitRow(line.text);
      const k = starts.indexOf(flat[i] - line.from);
      view.dispatch({ selection: EditorSelection.range(flat[i], flat[i] + (cells[k]?.length ?? 0)), scrollIntoView: true });
    } else if (dir > 0 && i < 0) {
      const row = emptyRow(state, t);
      view.dispatch({
        changes: { from: t.to, insert: "\n" + row },
        selection: EditorSelection.cursor(t.to + 1 + row.indexOf("|") + 2),
        scrollIntoView: true,
        userEvent: "input",
      });
    }
    return true;
  };

const enter: Command = (view) => {
  const { state } = view;
  const sel = state.selection.main;
  const line = state.doc.lineAt(sel.head);
  const t = tableAt(state, sel.head);
  if (t) {
    if (line.number === t.first + 1) return true;
    const row = emptyRow(state, t);
    view.dispatch({
      changes: { from: line.to, insert: "\n" + row },
      selection: EditorSelection.cursor(line.to + 1 + row.indexOf("|") + 2),
      scrollIntoView: true,
      userEvent: "input",
    });
    return true;
  }
  // "| a | b |" + Enter starts a table.
  const next = line.number < state.doc.lines ? state.doc.line(line.number + 1).text : "";
  if (ROW.test(line.text) && sel.head === line.to && !DELIM.test(next)) {
    const { cells } = splitRow(line.text);
    if (cells.length < 2 && !cells[0]) return false;
    const indent = /^\s*/.exec(line.text)![0];
    const delim = `${indent}|${cells.map(() => " --- |").join("")}`;
    const row = `${indent}|${cells.map(() => "   |").join("")}`;
    view.dispatch({
      changes: { from: line.to, insert: `\n${delim}\n${row}` },
      selection: EditorSelection.cursor(line.to + 2 + delim.length + indent.length + 2),
      scrollIntoView: true,
      userEvent: "input",
    });
    return true;
  }
  return false;
};

/** A click on a drawn table puts the cursor in the clicked cell. */
const clicks = EditorView.domEventHandlers({
  mousedown(e, view) {
    const target = e.target as HTMLElement;
    if (target.closest(".cm-link[data-href]")) return false;
    const cell = target.closest<HTMLElement>(".cm-table td, .cm-table th");
    if (!cell) return false;
    const t = tableAt(view.state, view.posAtDOM(cell));
    if (!t) return false;
    const line = view.state.doc.line(t.first + Number(cell.dataset.row));
    const { cells, starts } = splitRow(line.text);
    const c = Math.min(Number(cell.dataset.col), starts.length - 1);
    const at = line.from + (starts[c] ?? 0) + (cells[c]?.length ?? 0);
    view.focus();
    view.dispatch({ selection: EditorSelection.cursor(at) });
    e.preventDefault();
    return true;
  },
});

export const tableKeys = Prec.highest(
  keymap.of([
    { key: "Tab", run: nextCell(1), shift: nextCell(-1) },
    { key: "Enter", run: enter, shift: (v) => (tableAt(v.state, v.state.selection.main.head) ? enter(v) : false) },
  ]),
);

export const tablesExt = [tableField, EditorView.focusChangeEffect.of((_, focusing) => setFocus.of(focusing)), tidy, clicks, tableKeys];

// Fenced code blocks: a soft box in the mono font with syntax highlighting.
// The ``` fences are hidden until the cursor is in the block; the opening one
// becomes a small header with the language name and a copy button.
//
// Inside a block the keys behave like a code editor: Enter keeps the
// indentation, Tab indents, Backspace just deletes.

import { deleteCharBackward } from "@codemirror/commands";
import { syntaxTree } from "@codemirror/language";
import { classHighlighter } from "@lezer/highlight";
import { syntaxHighlighting } from "@codemirror/language";
import { EditorSelection, Prec, RangeSetBuilder, type EditorState, type Range } from "@codemirror/state";
import { Decoration, EditorView, ViewPlugin, WidgetType, keymap, type DecorationSet, type ViewUpdate, type Command } from "@codemirror/view";
import type { SyntaxNode } from "@lezer/common";

const TAB = "    ";

/** A fenced code block, with its parts as document positions. */
export interface Fence {
  from: number;
  to: number;
  /** First and last line numbers (1-based); `closed` if the last is a fence. */
  first: number;
  last: number;
  closed: boolean;
  lang: string;
  /** Indentation of the opening fence (the block's place in a list). */
  indent: number;
}

function fenceOf(state: EditorState, node: SyntaxNode): Fence {
  const first = state.doc.lineAt(node.from);
  const last = state.doc.lineAt(node.to);
  const marks = node.getChildren("CodeMark");
  const info = node.getChild("CodeInfo");
  return {
    from: node.from,
    to: node.to,
    first: first.number,
    last: last.number,
    closed: marks.length > 1 && last.number > first.number,
    lang: info ? state.sliceDoc(info.from, info.to).trim() : "",
    indent: /^ */.exec(first.text)![0].length,
  };
}

/** Every fenced block that overlaps from..to. */
export function fences(state: EditorState, from = 0, to = state.doc.length): Fence[] {
  const out: Fence[] = [];
  syntaxTree(state).iterate({
    from,
    to,
    enter: (n) => {
      if (n.name === "FencedCode") {
        out.push(fenceOf(state, n.node));
        return false;
      }
    },
  });
  return out;
}

/** The fenced block whose lines contain pos, if any. */
export function fenceAt(state: EditorState, pos: number): Fence | null {
  const line = state.doc.lineAt(pos);
  return fences(state, line.from, line.to).find((f) => line.number >= f.first && line.number <= f.last) ?? null;
}

/** The code inside a block, without the fences and the list indentation. */
export function codeOf(state: EditorState, f: Fence): string {
  const lines: string[] = [];
  const end = f.closed ? f.last - 1 : f.last;
  for (let n = f.first + 1; n <= end; n++) {
    const t = state.doc.line(n).text;
    lines.push(t.slice(Math.min(f.indent, /^ */.exec(t)![0].length)));
  }
  return lines.join("\n");
}

export async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text);
    return;
  } catch {
    // Not allowed here (some webviews): the old way.
  }
  const area = document.createElement("textarea");
  area.value = text;
  area.style.position = "fixed";
  area.style.opacity = "0";
  document.body.append(area);
  area.select();
  document.execCommand("copy");
  area.remove();
}

class HeaderWidget extends WidgetType {
  constructor(readonly lang: string) {
    super();
  }
  eq(o: HeaderWidget) {
    return o.lang === this.lang;
  }
  toDOM() {
    const el = document.createElement("span");
    el.className = "cm-cb-header";
    const lang = document.createElement("span");
    lang.className = "cm-cb-lang";
    lang.textContent = this.lang;
    const copy = document.createElement("button");
    copy.type = "button";
    copy.className = "cm-cb-copy";
    copy.textContent = "copy";
    copy.title = "Copy the code";
    el.append(lang, copy);
    return el;
  }
  ignoreEvent() {
    return false;
  }
}

const hidden = Decoration.replace({});
const fenceMark = Decoration.mark({ class: "cm-cb-fence" });

function touches(view: EditorView, f: Fence) {
  return view.hasFocus && view.state.selection.ranges.some((r) => r.from <= f.to && r.to >= f.from);
}

function build(view: EditorView) {
  const { state } = view;
  const deco: Range<Decoration>[] = [];
  const atomic = new RangeSetBuilder<Decoration>();
  const seen = new Set<number>();
  for (const { from, to } of view.visibleRanges) {
    for (const f of fences(state, from, to)) {
      if (seen.has(f.from)) continue;
      seen.add(f.from);
      const active = touches(view, f);
      const lvl = Math.min(Math.floor(f.indent / 2), 8);
      for (let n = f.first; n <= f.last; n++) {
        const line = state.doc.line(n);
        const head = n === f.first;
        const foot = f.closed && n === f.last;
        const cls = ["cm-cb", `cm-lvl-${lvl}`];
        if (head) cls.push("cm-cb-head");
        if (foot) cls.push("cm-cb-foot");
        if (n === f.last) cls.push("cm-cb-end");
        if (active) cls.push("is-editing");
        deco.push(Decoration.line({ class: cls.join(" ") }).range(line.from));
        // The list indentation is drawn as the box's offset.
        const lead = Math.min(f.indent, /^ */.exec(line.text)![0].length);
        const start = line.from + lead;
        if (lead) {
          deco.push(hidden.range(line.from, start));
          atomic.add(line.from, start, hidden);
        }
        if ((head || foot) && start < line.to) {
          if (active) deco.push(fenceMark.range(start, line.to));
          else if (head) deco.push(Decoration.replace({ widget: new HeaderWidget(f.lang) }).range(start, line.to));
          else deco.push(hidden.range(start, line.to));
        }
      }
    }
  }
  return { decorations: Decoration.set(deco, true), atomic: atomic.finish() };
}

const plugin = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;
    atomic: DecorationSet;
    constructor(view: EditorView) {
      ({ decorations: this.decorations, atomic: this.atomic } = build(view));
    }
    update(u: ViewUpdate) {
      if (u.docChanged || u.selectionSet || u.viewportChanged || u.focusChanged || syntaxTree(u.startState) !== syntaxTree(u.state)) {
        ({ decorations: this.decorations, atomic: this.atomic } = build(u.view));
      }
    }
  },
  {
    decorations: (v) => v.decorations,
    provide: (p) => EditorView.atomicRanges.of((view) => view.plugin(p)?.atomic ?? Decoration.none),
    eventHandlers: {
      mousedown(e, view) {
        const btn = (e.target as HTMLElement).closest<HTMLElement>(".cm-cb-copy");
        if (!btn) return false;
        const f = fenceAt(view.state, view.posAtDOM(btn));
        if (f) {
          void copyText(codeOf(view.state, f));
          btn.textContent = "copied";
          window.setTimeout(() => (btn.textContent = "copy"), 1200);
        }
        e.preventDefault();
        return true;
      },
    },
  },
);

/** The block around the cursor, when the cursor is between its fences. */
function bodyAt(state: EditorState): Fence | null {
  const sel = state.selection.main;
  const f = fenceAt(state, sel.head);
  if (!f) return null;
  const n = state.doc.lineAt(sel.head).number;
  return n > f.first && (!f.closed || n < f.last) ? f : null;
}

const OPENING = /^(\s*)(`{3,}|~{3,})[\w+#.-]*\s*$/;

const enter: Command = (view) => {
  const { state } = view;
  const sel = state.selection.main;
  const line = state.doc.lineAt(sel.head);
  // Enter after a new "```lang": add the closing fence too, so the rest of
  // the document doesn't turn into code while the block is being written.
  const open = OPENING.exec(line.text);
  if (open && sel.empty && sel.head === line.to) {
    const f = fenceAt(state, sel.head);
    if (f && f.first === line.number && !f.closed) {
      const pad = open[1];
      view.dispatch({
        changes: { from: line.to, insert: `\n${pad}\n${pad}${open[2]}` },
        selection: EditorSelection.cursor(line.to + 1 + pad.length),
        scrollIntoView: true,
        userEvent: "input",
      });
      return true;
    }
  }
  if (!bodyAt(state)) return false;
  view.dispatch(
    view.state.changeByRange((r) => {
      const line = view.state.doc.lineAt(r.from);
      const insert = "\n" + /^\s*/.exec(line.text)![0];
      return { changes: { from: r.from, to: r.to, insert }, range: EditorSelection.cursor(r.from + insert.length) };
    }),
    { scrollIntoView: true, userEvent: "input" },
  );
  return true;
};

const tab: Command = (view) => {
  if (!bodyAt(view.state)) return false;
  view.dispatch(view.state.replaceSelection(TAB), { userEvent: "input.indent" });
  return true;
};

const untab: Command = (view) => {
  if (!bodyAt(view.state)) return false;
  const line = view.state.doc.lineAt(view.state.selection.main.head);
  const f = fenceAt(view.state, line.from)!;
  const lead = /^ */.exec(line.text)![0].length;
  const n = Math.min(TAB.length, Math.max(0, lead - f.indent));
  if (n) view.dispatch({ changes: { from: line.from + f.indent, to: line.from + f.indent + n }, userEvent: "delete.dedent" });
  return true;
};

/** In the indentation, one Backspace takes back one Tab. */
const backspace: Command = (view) => {
  const f = bodyAt(view.state);
  if (!f) return false;
  const sel = view.state.selection.main;
  const line = view.state.doc.lineAt(sel.head);
  const before = line.text.slice(0, sel.head - line.from);
  if (sel.empty && /^ +$/.test(before) && before.length > f.indent) {
    const col = before.length - f.indent;
    const n = col % TAB.length || TAB.length;
    view.dispatch({ changes: { from: sel.head - n, to: sel.head }, userEvent: "delete.backward" });
    return true;
  }
  return deleteCharBackward(view);
};

/** Keys inside a code block; they go before the outliner's and markdown's. */
export const codeKeys = Prec.highest(
  keymap.of([
    { key: "Enter", run: enter, shift: (v) => (bodyAt(v.state) ? enter(v) : false) },
    { key: "Tab", run: tab, shift: untab },
    { key: "Backspace", run: backspace },
  ]),
);

export const codeBlocks = [plugin, syntaxHighlighting(classHighlighter), codeKeys];

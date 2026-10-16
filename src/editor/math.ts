// LaTeX math, drawn with KaTeX: $inline$ inside a sentence, $$display$$ as
// a centred equation (which may span several lines). A formula shows
// typeset until the cursor touches it; then its source shows for editing.
// Copying copies the source, since the document itself stays markdown.
//
// A lone dollar stays text ("5$", "$5 and $10"), following pandoc's rule: the
// opening $ is followed by a non-space, the closing $ follows a non-space and
// isn't followed by a digit.

import { syntaxTree } from "@codemirror/language";
import { EditorState, StateEffect, StateField, type Range } from "@codemirror/state";
import { Decoration, EditorView, WidgetType, type DecorationSet } from "@codemirror/view";
import katex from "katex";
import "katex/dist/katex.min.css";

export interface MathRange {
  from: number;
  to: number;
  tex: string;
  display: boolean;
}

const CODE = new Set(["InlineCode", "FencedCode", "CodeBlock"]);

/** Every formula in the document, outside code. */
export function findMath(state: EditorState): MathRange[] {
  const text = state.doc.toString();
  if (!text.includes("$")) return [];
  const code: [number, number][] = [];
  syntaxTree(state).iterate({
    enter: (n) => {
      if (CODE.has(n.name)) {
        code.push([n.from, n.to]);
        return false;
      }
    },
  });
  const inCode = (p: number) => code.some(([a, b]) => p >= a && p < b);

  const found: MathRange[] = [];
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (c === "\\") {
      i++;
      continue;
    }
    if (c !== "$" || inCode(i)) continue;
    if (text[i + 1] === "$") {
      // Display: up to the next $$ outside code, but not across an empty line.
      let end = text.indexOf("$$", i + 2);
      while (end > 0 && inCode(end)) end = text.indexOf("$$", end + 2);
      const tex = end < 0 ? "" : text.slice(i + 2, end);
      if (end > 0 && tex.trim() && !/\n\s*\n/.test(tex)) {
        found.push({ from: i, to: end + 2, tex, display: true });
        i = end + 1;
      } else {
        i++;
      }
      continue;
    }
    if (/\s/.test(text[i + 1] ?? " ")) continue;
    for (let k = i + 1; k < text.length && text[k] !== "\n"; k++) {
      if (text[k] === "\\") {
        k++;
        continue;
      }
      if (text[k] !== "$" || inCode(k)) continue;
      if (/\s/.test(text[k - 1]) || /\d/.test(text[k + 1] ?? "") || text[k + 1] === "$") break;
      found.push({ from: i, to: k + 1, tex: text.slice(i + 1, k), display: false });
      i = k;
      break;
    }
  }
  return found;
}

class MathWidget extends WidgetType {
  constructor(readonly tex: string, readonly display: boolean) {
    super();
  }
  eq(o: MathWidget) {
    return o.tex === this.tex && o.display === this.display;
  }
  toDOM() {
    const el = document.createElement("span");
    el.className = this.display ? "cm-math cm-math-display" : "cm-math";
    katex.render(this.tex, el, { displayMode: this.display, throwOnError: false, output: "htmlAndMathml" });
    return el;
  }
  ignoreEvent() {
    return false;
  }
}

const setFocus = StateEffect.define<boolean>();

interface MathState {
  ranges: MathRange[];
  focused: boolean;
  deco: DecorationSet;
}

function decorate(state: EditorState, ranges: MathRange[], focused: boolean): DecorationSet {
  const sel = state.selection.ranges;
  const touched = (m: MathRange) => focused && sel.some((r) => r.from <= m.to && r.to >= m.from);
  const deco: Range<Decoration>[] = [];
  for (const m of ranges) {
    if (touched(m)) deco.push(Decoration.mark({ class: "cm-math-src" }).range(m.from, m.to));
    else deco.push(Decoration.replace({ widget: new MathWidget(m.tex, m.display) }).range(m.from, m.to));
  }
  return Decoration.set(deco, true);
}

/** The formulas of the document, for other extensions to steer clear of. */
export const mathField = StateField.define<MathState>({
  create(state) {
    const ranges = findMath(state);
    return { ranges, focused: false, deco: decorate(state, ranges, false) };
  },
  update(v, tr) {
    let { ranges, focused } = v;
    for (const e of tr.effects) if (e.is(setFocus)) focused = e.value;
    const reparsed = syntaxTree(tr.startState) !== syntaxTree(tr.state);
    if (tr.docChanged || reparsed) ranges = findMath(tr.state);
    if (ranges === v.ranges && focused === v.focused && !tr.selection) return v;
    return { ranges, focused, deco: decorate(tr.state, ranges, focused) };
  },
  provide: (f) => EditorView.decorations.from(f, (v) => v.deco),
});

export function insideMath(state: EditorState, from: number, to: number) {
  const f = state.field(mathField, false);
  return !!f && f.ranges.some((m) => from >= m.from && to <= m.to);
}

export const math = [mathField, EditorView.focusChangeEffect.of((_, focusing) => setFocus.of(focusing))];

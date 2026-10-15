// Live preview: the document stays plain markdown, but it is drawn styled.
//
// - List structure (indent, "- ", "- [ ] ") is ALWAYS drawn as a widget (a
//   bullet or a checkbox) and is atomic for the cursor, so raw list syntax
//   never shows. Tab/Shift-Tab/Backspace edit it (see keys.ts).
// - Inline syntax (**bold**, _em_, `code`, ~~strike~~, [links](url)) and
//   heading hashes are hidden on every line except the ones holding the
//   cursor, where they show dimmed so they can be edited.

import { syntaxTree } from "@codemirror/language";
import { RangeSetBuilder, type EditorState, type Range } from "@codemirror/state";
import { Decoration, EditorView, ViewPlugin, WidgetType, type DecorationSet, type ViewUpdate } from "@codemirror/view";

/** One line's block structure, parsed with a regex (cheap, and exactly the
 *  subset of markdown we write). */
export interface LineInfo {
  /** Nesting level: leading spaces / 2. */
  level: number;
  kind: "task" | "bullet" | "heading" | "text";
  /** Offset (within the line) where the visible content starts. */
  contentStart: number;
  /** Task only: offset of the char inside "[ ]". */
  checkAt: number;
  done: boolean;
  headingLevel: number;
}

const LIST = /^(\s*)([-*+]) (?:\[([ xX])\] )?/;
const HEADING = /^(#{1,6}) /;

export function lineInfo(text: string): LineInfo {
  const m = LIST.exec(text);
  if (m) {
    const level = Math.floor(m[1].replace(/\t/g, "  ").length / 2);
    if (m[3] !== undefined) {
      return { level, kind: "task", contentStart: m[0].length, checkAt: m[1].length + 3, done: m[3] !== " ", headingLevel: 0 };
    }
    return { level, kind: "bullet", contentStart: m[0].length, checkAt: -1, done: false, headingLevel: 0 };
  }
  const h = HEADING.exec(text);
  if (h) return { level: 0, kind: "heading", contentStart: h[0].length, checkAt: -1, done: false, headingLevel: h[1].length };
  const lead = /^\s*/.exec(text)![0].length;
  return { level: Math.floor(lead / 2), kind: "text", contentStart: lead, checkAt: -1, done: false, headingLevel: 0 };
}

class MarkerWidget extends WidgetType {
  constructor(readonly kind: "task" | "bullet", readonly done: boolean, readonly checkPos: number) {
    super();
  }
  eq(o: MarkerWidget) {
    return o.kind === this.kind && o.done === this.done && o.checkPos === this.checkPos;
  }
  toDOM() {
    const wrap = document.createElement("span");
    wrap.className = "cm-marker";
    const box = document.createElement("span");
    if (this.kind === "task") {
      box.className = "cm-check" + (this.done ? " is-done" : "");
      box.dataset.pos = String(this.checkPos);
      box.setAttribute("role", "checkbox");
      box.setAttribute("aria-checked", String(this.done));
    } else {
      box.className = "cm-bullet";
    }
    wrap.appendChild(box);
    return wrap;
  }
  ignoreEvent() {
    return false;
  }
}

const hidden = Decoration.replace({});
const dim = Decoration.mark({ class: "cm-syntax" });
const INLINE: Record<string, string> = {
  StrongEmphasis: "cm-strong",
  Emphasis: "cm-em",
  InlineCode: "cm-code",
  Strikethrough: "cm-strike",
};
const MARKS = new Set(["EmphasisMark", "CodeMark", "StrikethroughMark", "LinkMark", "LinkTitle"]);

/** Line numbers that hold a cursor or selection, only while focused. */
function activeLines(view: EditorView): Set<number> {
  const lines = new Set<number>();
  if (!view.hasFocus) return lines;
  for (const r of view.state.selection.ranges) {
    const a = view.state.doc.lineAt(r.from).number;
    const b = view.state.doc.lineAt(r.to).number;
    for (let n = a; n <= b; n++) lines.add(n);
  }
  return lines;
}

function build(view: EditorView): { decorations: DecorationSet; atomic: DecorationSet } {
  const { state } = view;
  const active = activeLines(view);
  const deco: Range<Decoration>[] = [];
  const atomic = new RangeSetBuilder<Decoration>();

  for (const { from, to } of view.visibleRanges) {
    // Block structure, line by line.
    for (let pos = from; pos <= to; ) {
      const line = state.doc.lineAt(pos);
      const info = lineInfo(line.text);
      const isActive = active.has(line.number);
      const cls = ["cm-ln", `cm-lvl-${Math.min(info.level, 8)}`, `cm-ln-${info.kind}`];

      if (info.kind === "task" || info.kind === "bullet") {
        const marker = Decoration.replace({ widget: new MarkerWidget(info.kind, info.done, line.from + info.checkAt) });
        deco.push(marker.range(line.from, line.from + info.contentStart));
        atomic.add(line.from, line.from + info.contentStart, marker);
        if (info.done && line.to > line.from + info.contentStart) {
          deco.push(Decoration.mark({ class: "cm-done" }).range(line.from + info.contentStart, line.to));
        }
      } else if (info.kind === "heading") {
        cls.push(`cm-h${Math.min(info.headingLevel, 3)}`);
        deco.push((isActive ? dim : hidden).range(line.from, line.from + info.contentStart));
      } else if (info.contentStart > 0) {
        // Indentation is drawn as padding (cm-lvl-N), never as raw spaces.
        deco.push(hidden.range(line.from, line.from + info.contentStart));
        atomic.add(line.from, line.from + info.contentStart, hidden);
      }
      deco.push(Decoration.line({ class: cls.join(" ") }).range(line.from));
      pos = line.to + 1;
    }

    // Inline syntax from the markdown parse tree.
    syntaxTree(state).iterate({
      from,
      to,
      enter: (node) => {
        const name = node.name;
        const isActive = active.has(state.doc.lineAt(node.from).number);
        if (INLINE[name]) {
          deco.push(Decoration.mark({ class: INLINE[name] }).range(node.from, node.to));
        } else if (name === "Link") {
          const url = node.node.getChild("URL");
          const href = url ? state.sliceDoc(url.from, url.to) : "";
          deco.push(linkMark(href, isActive).range(node.from, node.to));
        } else if (name === "URL") {
          if (node.node.parent?.name === "Link") {
            deco.push((isActive ? dim : hidden).range(node.from, node.to));
          } else {
            // Bare autolink: the text is the address.
            deco.push(linkMark(state.sliceDoc(node.from, node.to), isActive).range(node.from, node.to));
          }
          return false;
        } else if (MARKS.has(name)) {
          if (node.to > node.from) deco.push((isActive ? dim : hidden).range(node.from, node.to));
        }
      },
    });
  }

  return { decorations: Decoration.set(deco, true), atomic: atomic.finish() };
}

function linkMark(href: string, active: boolean) {
  // Only inactive links carry the address: a click there opens it, while on
  // the line being edited a click just places the cursor.
  return Decoration.mark({ class: "cm-link", attributes: active ? {} : { "data-href": href } });
}

export function toggleTask(state: EditorState, checkPos: number) {
  const done = state.sliceDoc(checkPos, checkPos + 1) !== " ";
  return { changes: { from: checkPos, to: checkPos + 1, insert: done ? " " : "x" } };
}

export function livePreview(open: (url: string) => void) {
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
          const t = e.target as HTMLElement;
          const check = t.closest<HTMLElement>(".cm-check");
          if (check) {
            view.dispatch(toggleTask(view.state, Number(check.dataset.pos)));
            e.preventDefault();
            return true;
          }
          const link = t.closest<HTMLElement>(".cm-link[data-href]");
          if (link && link.dataset.href) {
            open(link.dataset.href);
            e.preventDefault();
            return true;
          }
          return false;
        },
      },
    },
  );
  return plugin;
}

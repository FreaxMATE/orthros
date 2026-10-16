import { history, historyKeymap, defaultKeymap } from "@codemirror/commands";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { EditorSelection, EditorState, Transaction } from "@codemirror/state";
import { EditorView, drawSelection, keymap, placeholder } from "@codemirror/view";
import { api, openUrl } from "../api";
import { boundaryKeys, outlinerKeys } from "./keys";
import { livePreview } from "./livePreview";

export interface DocEditor {
  view: EditorView;
  name: string;
  /** Write pending changes now. */
  flush(): Promise<void>;
  /** The document changed on disk (sync, capture, another editor). */
  remote(content: string): void;
  focusLine(line: number): void;
  focusEdge(edge: "start" | "end"): void;
}

export interface Options {
  parent: HTMLElement;
  name: string;
  doc: string;
  /** Inserted when an empty document gets focus ("- [ ] " for days), and
   *  removed again if it is left untouched. */
  seed: string;
  placeholder: string;
  /** Arrow off the first/last line; return true if focus moved. */
  onLeave?: (dir: -1 | 1) => boolean;
}

const SAVE_DELAY = 400;

// Through CodeMirror's theme: its base theme outranks plain CSS for these
// (monospace font, a thin black cursor that vanishes on dark backgrounds).
const theme = EditorView.theme({
  ".cm-scroller": { fontFamily: "var(--serif)", lineHeight: "1.55", overflow: "visible" },
  ".cm-content": { caretColor: "var(--caret)" },
  ".cm-cursor, .cm-dropCursor": { borderLeft: "2px solid var(--caret)", marginLeft: "-1px" },
  ".cm-placeholder": { color: "var(--faint)", fontStyle: "italic" },
  ".cm-line": { padding: "3px 0" },
});

/** Replace the editor's text with `next`, touching only the part that
 *  differs so the cursor stays put. Not an edit: no save, no undo step. */
function replaceAll(view: EditorView, next: string) {
  const cur = view.state.doc.toString();
  if (cur === next) return;
  let a = 0;
  while (a < cur.length && a < next.length && cur[a] === next[a]) a++;
  let b = 0;
  while (b < cur.length - a && b < next.length - a && cur[cur.length - 1 - b] === next[next.length - 1 - b]) b++;
  view.dispatch({
    changes: { from: a, to: cur.length - b, insert: next.slice(a, next.length - b) },
    annotations: [Transaction.remote.of(true), Transaction.addToHistory.of(false)],
  });
}

export function createEditor(o: Options): DocEditor {
  let timer: number | undefined;
  // The on-disk version the editor's text is based on. Saves send it along,
  // so Rust can merge if the file changed underneath us meanwhile.
  let basis = o.doc;
  // What this editor saved most recently, to recognise stale echoes.
  const recent: string[] = [];
  const remember = (t: string) => {
    recent.push(t);
    if (recent.length > 20) recent.shift();
  };

  /** The editor's text, with an untouched seed counting as empty. */
  const text = () => {
    const t = view.state.doc.toString();
    return t.trim() === o.seed.trim() ? "" : t;
  };

  const flush = async () => {
    window.clearTimeout(timer);
    timer = undefined;
    const sent = text();
    if (sent === basis) return;
    remember(sent);
    const disk = await api.save(o.name, sent, basis);
    if (text() === sent) {
      // Nothing typed while saving: show the merge result, if any.
      if (disk !== sent) replaceAll(view, disk);
      basis = disk;
    } else {
      // Typed meanwhile: that text builds on what we sent. The pending save
      // will merge it with whatever is on disk now.
      basis = sent;
    }
  };

  const remote = (content: string) => {
    // A late echo of one of our own earlier saves, not a new version:
    // showing it would briefly undo the latest typing (and move the cursor).
    if (content !== text() && recent.includes(content)) return;
    if (content === text()) {
      basis = content;
    } else if (timer === undefined && text() === basis) {
      // No local edits pending: just show the new version.
      replaceAll(view, content);
      basis = content;
    }
    // Otherwise our pending save merges with disk, so nothing is lost.
  };

  const view = new EditorView({
    parent: o.parent,
    state: EditorState.create({
      doc: o.doc,
      extensions: [
        theme,
        drawSelection(),
        history(),
        EditorView.lineWrapping,
        markdown({ base: markdownLanguage }),
        livePreview(openUrl),
        outlinerKeys,
        o.onLeave ? boundaryKeys(o.onLeave) : [],
        keymap.of([...historyKeymap, ...defaultKeymap]),
        placeholder(o.placeholder),
        EditorView.contentAttributes.of({ spellcheck: "true", "aria-label": o.placeholder }),
        EditorView.updateListener.of((u) => {
          if (u.docChanged && !u.transactions.some((t) => t.annotation(Transaction.remote))) {
            window.clearTimeout(timer);
            timer = window.setTimeout(flush, SAVE_DELAY);
          }
          if (u.focusChanged) {
            // No dispatching from inside an update: defer until it's done.
            queueMicrotask(() => {
              const s = view.state;
              if (view.hasFocus && s.doc.length === 0 && o.seed) {
                view.dispatch({ changes: { from: 0, insert: o.seed }, selection: EditorSelection.cursor(o.seed.length) });
              } else if (!view.hasFocus) {
                if (s.doc.toString() === o.seed) view.dispatch({ changes: { from: 0, to: s.doc.length } });
                void flush();
              }
            });
          }
        }),
      ],
    }),
  });

  return {
    view,
    name: o.name,
    flush,
    remote,
    focusLine(line) {
      const l = view.state.doc.line(Math.min(Math.max(line + 1, 1), view.state.doc.lines));
      view.focus();
      view.dispatch({ selection: EditorSelection.cursor(l.to), scrollIntoView: true });
    },
    focusEdge(edge) {
      view.focus();
      const pos = edge === "start" ? 0 : view.state.doc.length;
      view.dispatch({ selection: EditorSelection.cursor(pos), scrollIntoView: true });
    },
  };
}

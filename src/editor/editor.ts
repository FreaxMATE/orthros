import { history, historyKeymap, defaultKeymap } from "@codemirror/commands";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { EditorSelection, EditorState } from "@codemirror/state";
import { EditorView, keymap, placeholder } from "@codemirror/view";
import { api, openUrl } from "../api";
import { boundaryKeys, outlinerKeys } from "./keys";
import { livePreview } from "./livePreview";

export interface DocEditor {
  view: EditorView;
  name: string;
  /** Write pending changes now. */
  flush(): Promise<void>;
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

// Through CodeMirror's theme so it outranks the base theme's monospace.
const theme = EditorView.theme({
  ".cm-scroller": { fontFamily: "var(--serif)", lineHeight: "1.55", overflow: "visible" },
});

export function createEditor(o: Options): DocEditor {
  let timer: number | undefined;
  let saved = o.doc;

  const flush = async () => {
    window.clearTimeout(timer);
    timer = undefined;
    let text = view.state.doc.toString();
    if (text.trim() === o.seed.trim()) text = "";
    if (text === saved) return;
    saved = text;
    await api.write(o.name, text);
  };

  const view = new EditorView({
    parent: o.parent,
    state: EditorState.create({
      doc: o.doc,
      extensions: [
        theme,
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
          if (u.docChanged) {
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

// Languages for fenced code blocks: CodeMirror's collection (about 140,
// each loaded only when a block uses it), plus Nix, which it lacks.

import { LanguageDescription, LanguageSupport, StreamLanguage, type StreamParser } from "@codemirror/language";
import { languages } from "@codemirror/language-data";

interface NixState {
  /** Inside a string ('"' or "''"), or a block comment. */
  inside: '"' | "''" | "/*" | null;
}

/** Consume up to the end of the current string or comment. */
function until(stream: Parameters<StreamParser<NixState>["token"]>[0], st: NixState) {
  const close = st.inside === "/*" ? "*/" : st.inside!;
  while (!stream.eol()) {
    if (st.inside === '"' && stream.eat("\\")) {
      stream.next();
      continue;
    }
    if (st.inside === "''" && stream.match("'''")) continue;
    if (stream.match(close)) {
      st.inside = null;
      return;
    }
    stream.next();
  }
}

const nix: StreamParser<NixState> = {
  name: "nix",
  startState: () => ({ inside: null }),
  token(stream, st) {
    if (st.inside) {
      const style = st.inside === "/*" ? "comment" : "string";
      until(stream, st);
      return style;
    }
    if (stream.eatSpace()) return null;
    if (stream.match("#")) {
      stream.skipToEnd();
      return "comment";
    }
    for (const open of ['"', "''", "/*"] as const) {
      if (stream.match(open)) {
        st.inside = open;
        until(stream, st);
        return open === "/*" ? "comment" : "string";
      }
    }
    if (stream.match(/^(let|in|with|rec|inherit|if|then|else|assert|or)\b/)) return "keyword";
    if (stream.match(/^(true|false|null)\b/)) return "atom";
    if (stream.match(/^(import|builtins|throw|abort|derivation)\b/)) return "builtin";
    if (stream.match(/^\d+(\.\d+)?/)) return "number";
    if (stream.match(/^(~|\.{1,2})?\/[\w.+\-/]+/) || stream.match(/^<[\w.+\-/]+>/)) return "string";
    if (stream.match(/^[A-Za-z_][\w'-]*(?=\s*=(?!=))/)) return "propertyName";
    if (stream.match(/^[A-Za-z_][\w'-]*/)) return "variableName";
    stream.next();
    return null;
  },
  languageData: { commentTokens: { line: "#", block: { open: "/*", close: "*/" } } },
};

export const codeLanguages: LanguageDescription[] = [
  ...languages,
  LanguageDescription.of({
    name: "Nix",
    alias: ["nix"],
    extensions: ["nix"],
    load: async () => new LanguageSupport(StreamLanguage.define(nix)),
  }),
];

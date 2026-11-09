// Places in notes: pure text functions, no DOM and no network, so the rules
// are easy to check by reading. A place is a link on a line; the line's
// checkbox says planned or visited. The map module does the rest.

export interface Doc {
  /** Document name: "later" or "notes/<slug>". */
  name: string;
  title: string;
  text: string;
}

export interface Link {
  doc: string;
  title: string;
  /** 0-based line in the document. */
  line: number;
  url: string;
  /** Link text, else the line without the URL, else null. */
  label: string | null;
  /** The line is a ticked task: `- [x] ...`. */
  visited: boolean;
}

export interface Entry {
  line: number;
  text: string;
  visited: boolean;
}

const MD_LINK = /\[([^\]]*)\]\(([^)\s]+)(?:\s+"[^"]*")?\)/g;
const BARE_URL = /(?:https?:\/\/|geo:)[^\s<>"]+/g;
const TICKED = /^\s*(?:[-*+]|\d+[.)])\s+\[[xX]\]/;
const MARKER = /^\s*(?:(?:[-*+]|\d+[.)])\s+)?(?:\[[ xX]?\]\s*)?/;

export function isVisited(line: string): boolean {
  return TICKED.test(line);
}

const LIST_START = /^(\s*)([-*+]|\d+[.)]) (?:\[[ xX]?\] )?/;

/** The line with its checkbox set: ticked = been there, unticked = want to
 *  go. A plain bullet or plain text becomes a task. */
export function withVisited(line: string, visited: boolean): string {
  const box = visited ? "[x] " : "[ ] ";
  const m = LIST_START.exec(line);
  if (m) return `${m[1]}${m[2]} ${box}${line.slice(m[0].length)}`;
  const indent = /^\s*/.exec(line)![0];
  return `${indent}- ${box}${line.slice(indent.length)}`;
}

/** The document with the checkbox of the place's line set, or null if the
 *  link is gone. Looks for the link elsewhere if the line has moved. */
export function setVisited(text: string, line: number, url: string, visited: boolean): string | null {
  const lines = text.split("\n");
  const at = lines[line]?.includes(url) ? line : lines.findIndex((l) => l.includes(url));
  if (at < 0) return null;
  lines[at] = withVisited(lines[at], visited);
  return lines.join("\n");
}

/** A line as plain text: no list or task marker, links reduced to their
 *  text, trimmed. */
export function stripMarkup(line: string): string {
  return line
    .replace(MARKER, "")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .trim();
}

/** Every link on every line of every document (code fences skipped). */
export function extractLinks(docs: Doc[]): Link[] {
  const out: Link[] = [];
  for (const d of docs) {
    let fenced = false;
    d.text.split("\n").forEach((line, i) => {
      if (/^\s*(```|~~~)/.test(line)) fenced = !fenced;
      if (fenced) return;
      const visited = isVisited(line);
      const add = (url: string, label: string | null) =>
        out.push({ doc: d.name, title: d.title, line: i, url, label: label?.trim() || null, visited });
      // Markdown links first; blank them out so their URLs are not found
      // again as bare URLs.
      const found: [string, string | null][] = [];
      const rest = line.replace(MD_LINK, (_m, text: string, url: string) => {
        found.push([url, text]);
        return " ";
      });
      const bare = rest.match(BARE_URL) ?? [];
      const plain = stripMarkup(bare.reduce((s, u) => s.replace(u, " "), rest)).replace(/\s+/g, " ").trim() || null;
      for (const [url, text] of found) add(url, text || plain);
      for (const url of bare) add(url.replace(/[.,;:!?]+$/, ""), plain);
    });
  }
  return out;
}

/** The task and bullet lines of the "Been there" note (not the heading). */
export function beenThere(text: string): Entry[] {
  const out: Entry[] = [];
  text.split("\n").forEach((line, i) => {
    if (/^\s*#/.test(line) || !line.trim()) return;
    out.push({ line: i, text: line, visited: isVisited(line) });
  });
  return out;
}

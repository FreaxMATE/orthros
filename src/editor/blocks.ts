// Moving items: a block is a line plus every following line indented deeper
// (its sub-items). A fenced code block or a table counts as one line here:
// it moves whole and is never split. All functions work on the document
// split into lines and return new arrays; editors apply the result as one edit.

import { lineInfo } from "./livePreview";

export type Lines = string[];

/** Nesting level of a line, or -1 for an empty line (which ends blocks). */
export function level(text: string): number {
  return text.trim() === "" ? -1 : lineInfo(text).level;
}

const FENCE = /^\s*(`{3,}|~{3,})/;
const TABLE_ROW = /^\s*\|/;
const TABLE_DELIM = /^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?\s*$/;

/** Code blocks and tables as [first, last] line index pairs. */
export function units(lines: Lines): [number, number][] {
  const out: [number, number][] = [];
  for (let i = 0; i < lines.length; i++) {
    const open = FENCE.exec(lines[i]);
    if (open) {
      const mark = open[1];
      let j = i + 1;
      while (j < lines.length) {
        const close = FENCE.exec(lines[j]);
        if (close && close[1][0] === mark[0] && close[1].length >= mark.length && lines[j].trim() === close[1]) break;
        j++;
      }
      j = Math.min(j, lines.length - 1);
      out.push([i, j]);
      i = j;
    } else if (TABLE_ROW.test(lines[i]) && i + 1 < lines.length && TABLE_DELIM.test(lines[i + 1])) {
      let j = i + 1;
      while (j + 1 < lines.length && TABLE_ROW.test(lines[j + 1])) j++;
      out.push([i, j]);
      i = j;
    }
  }
  return out;
}

/** The code block or table that line i belongs to, if any. */
export function unitAt(lines: Lines, i: number): [number, number] | null {
  return units(lines).find(([s, e]) => i >= s && i <= e) ?? null;
}

/** Line i, or the first line of the code block or table it is part of. */
export function unitStart(lines: Lines, i: number): number {
  return unitAt(lines, i)?.[0] ?? i;
}

/** Index of the last line of the block starting at line i. */
export function blockEnd(lines: Lines, i: number): number {
  const us = units(lines);
  const own = us.find(([s, e]) => i >= s && i <= e);
  const base = level(lines[own ? own[0] : i]);
  let j = own ? own[1] : i;
  while (j + 1 < lines.length && level(lines[j + 1]) > base) {
    const next = us.find(([s]) => s === j + 1);
    j = next ? next[1] : j + 1;
  }
  return j;
}

function reindent(block: Lines, delta: number): Lines {
  if (delta === 0) return block;
  return block.map((l) => {
    if (l.trim() === "") return l;
    if (delta > 0) return "  ".repeat(delta) + l;
    const lead = /^ */.exec(l)![0].length;
    return l.slice(Math.min(lead, -delta * 2));
  });
}

/** Where a block may go: never after a document's final empty line. */
function clampSlot(lines: Lines, k: number): number {
  if (k > 0 && k >= lines.length && lines[lines.length - 1] === "") return lines.length - 1;
  return Math.max(0, Math.min(k, lines.length));
}

/** Insert `block` before line k of `target`, indented to fit there: like the
 *  line below the drop point, at most one level deeper than the line above. */
export function insertBlock(target: Lines, k: number, block: Lines): { lines: Lines; start: number } {
  k = clampSlot(target, k);
  const u = unitAt(target, k);
  if (u && k > u[0]) k = k - u[0] < u[1] + 1 - k ? u[0] : u[1] + 1;
  const below = k < target.length ? level(target[k]) : -1;
  const above = k > 0 ? level(target[k - 1]) : -1;
  let base = below >= 0 ? below : Math.max(above, 0);
  base = above >= 0 ? Math.min(base, above + 1) : 0;
  const moved = reindent(block, base - Math.max(level(block[0]), 0));
  const lines = [...target.slice(0, k), ...moved, ...target.slice(k)];
  return { lines, start: k };
}

/** Lines i..j taken out. A document never ends up with no line at all. */
export function removeBlock(lines: Lines, i: number, j: number): Lines {
  const rest = [...lines.slice(0, i), ...lines.slice(j + 1)];
  return rest.length ? rest : [""];
}

/** Move lines i..j to slot k of the same document (k counted before removal).
 *  Returns null when that wouldn't change anything. */
export function moveWithin(lines: Lines, i: number, j: number, k: number): { lines: Lines; start: number } | null {
  // Never into the middle of a code block or table.
  const u = unitAt(lines, k);
  if (u && k > u[0]) k = k - u[0] < u[1] + 1 - k ? u[0] : u[1] + 1;
  if (k >= i && k <= j + 1) return null;
  const block = lines.slice(i, j + 1);
  const rest = removeBlock(lines, i, j);
  return insertBlock(rest, k > j ? k - (j - i + 1) : k, block);
}

/** Alt+Up / Alt+Down: swap the block at line i with its neighbouring sibling
 *  block. "out" when it is already first/last in the document (it can then
 *  move to the neighbouring day), null when a parent is in the way. */
export function moveStep(lines: Lines, i: number, dir: -1 | 1): { lines: Lines; start: number } | "out" | null {
  i = unitStart(lines, i);
  const j = blockEnd(lines, i);
  const lvl = level(lines[i]);
  const block = lines.slice(i, j + 1);
  if (dir < 0) {
    if (i === 0) return "out";
    let p = unitStart(lines, i - 1);
    while (p > 0 && level(lines[p]) > lvl) p = unitStart(lines, p - 1);
    if (level(lines[p]) >= 0 && level(lines[p]) < lvl) return null; // that's our parent
    const prev = lines.slice(p, i);
    return { lines: [...lines.slice(0, p), ...block, ...prev, ...lines.slice(j + 1)], start: p };
  }
  const n = j + 1;
  const last = lines[lines.length - 1] === "" ? lines.length - 1 : lines.length;
  if (n >= last) return "out";
  if (level(lines[n]) >= 0 && level(lines[n]) < lvl) return null; // leaving our parent
  const m = level(lines[n]) < 0 ? n : blockEnd(lines, n);
  const next = lines.slice(n, m + 1);
  return { lines: [...lines.slice(0, i), ...next, ...block, ...lines.slice(m + 1)], start: i + next.length };
}

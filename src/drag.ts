// Drag items with the mouse: hover a line, grab the grip that appears to its
// left, drop it between lines of any visible day, or on the "Later" tab.
// Sub-items travel with their parent. Esc cancels.

import { blockEnd, insertBlock, moveWithin, removeBlock, type Lines } from "./editor/blocks";
import type { DocEditor } from "./editor/editor";

export interface DragTargets {
  /** Every editor currently shown (the day stream, or Later). */
  editors(): DocEditor[];
  /** Append a block to Later (when dropped on the Later tab). */
  toLater(block: Lines): void;
  laterTab: HTMLElement;
}

interface Slot {
  editor: DocEditor;
  /** Insert before this line (0-based); may equal the line count. */
  line: number;
  /** Where to draw the drop line, in viewport coordinates. */
  y: number;
  left: number;
  right: number;
}

const EDGE = 70;
/** Hover reaches this far left of the list. */
const REACH = 60;
/** Space between the grip and the checkbox. */
const GAP = 6;

/** The line under viewport point (x, y): anywhere from REACH left of the
 *  list to the right edge of the column. */
function lineAt(editors: DocEditor[], x: number, y: number) {
  for (const editor of editors) {
    const view = editor.view;
    const rect = view.contentDOM.getBoundingClientRect();
    const column = (view.dom.closest("main") ?? view.dom).getBoundingClientRect();
    if (y < rect.top || y > rect.bottom || x < rect.left - REACH || x > Math.max(column.right, rect.right)) continue;
    const block = view.lineBlockAtHeight(y - view.documentTop);
    return { editor, line: view.state.doc.lineAt(block.from) };
  }
  return null;
}

/** Where the grip attaches: the left edge of the checkbox (or bullet), centred
 *  on it; for other lines, the start of the text on the first row. */
function anchor(editor: DocEditor, from: number, text: string) {
  const view = editor.view;
  const { node } = view.domAtPos(from);
  const el = node instanceof HTMLElement ? node : node.parentElement;
  const lineEl = el?.closest(".cm-line");
  const marker = lineEl?.querySelector(".cm-marker");
  if (marker) {
    const r = marker.getBoundingClientRect();
    if (r.width) return { x: r.left, y: r.top + r.height / 2 };
  }
  const start = from + /^\s*/.exec(text)![0].length;
  const c = view.coordsAtPos(start, 1);
  return c ? { x: c.left, y: (c.top + c.bottom) / 2 } : null;
}

/** The slot nearest to viewport point y in `editor`. */
function slotIn(editor: DocEditor, y: number): Slot {
  const view = editor.view;
  const rect = view.contentDOM.getBoundingClientRect();
  const doc = view.state.doc;
  if (doc.length === 0) return { editor, line: 0, y: rect.top + 2, left: rect.left, right: rect.right };
  const pos = view.posAtCoords({ x: rect.left + 4, y: Math.min(Math.max(y, rect.top + 1), rect.bottom - 1) }, false);
  const line = doc.lineAt(pos);
  const top = view.coordsAtPos(line.from)?.top ?? rect.top;
  const block = view.lineBlockAt(line.from);
  const bottom = top + block.height;
  const after = y > (top + bottom) / 2;
  return { editor, line: line.number - 1 + (after ? 1 : 0), y: after ? bottom : top, left: rect.left, right: rect.right };
}

export function enableDrag(t: DragTargets) {
  const grip = document.createElement("div");
  grip.className = "drag-grip";
  grip.setAttribute("aria-hidden", "true");
  grip.innerHTML = '<svg viewBox="0 0 10 16" width="10" height="16"><circle cx="2.5" cy="3" r="1.3"/><circle cx="7.5" cy="3" r="1.3"/><circle cx="2.5" cy="8" r="1.3"/><circle cx="7.5" cy="8" r="1.3"/><circle cx="2.5" cy="13" r="1.3"/><circle cx="7.5" cy="13" r="1.3"/></svg>';
  grip.title = "Drag to move (Alt+Up/Down with the keyboard)";
  document.body.append(grip);

  const marker = document.createElement("div");
  marker.className = "drop-line";
  document.body.append(marker);

  let hovered: { editor: DocEditor; line: number } | null = null;
  let hideTimer: number | undefined;

  const hideGrip = () => {
    grip.classList.remove("is-shown");
    hovered = null;
  };

  // Show the grip next to the hovered line. The hover strip spans the whole
  // column plus a margin on the left, so the pointer can travel up and down
  // beside the list.
  document.addEventListener("mousemove", (e) => {
    if (dragging) return;
    if (e.target === grip) {
      window.clearTimeout(hideTimer);
      return;
    }
    const found = lineAt(t.editors(), e.clientX, e.clientY);
    if (!found) {
      window.clearTimeout(hideTimer);
      hideTimer = window.setTimeout(hideGrip, 250);
      return;
    }
    window.clearTimeout(hideTimer);
    const { editor, line } = found;
    if (line.text.trim() === "") return hideGrip();
    const at = anchor(editor, line.from, line.text);
    if (!at) return hideGrip();
    grip.style.transform = `translate(${at.x - GAP - 14 + window.scrollX}px, ${at.y + window.scrollY - 11}px)`;
    grip.classList.add("is-shown");
    hovered = { editor, line: line.number - 1 };
  });

  // Dragging.
  let dragging: { editor: DocEditor; i: number; j: number } | null = null;
  let slot: Slot | null = null;
  let overLater = false;
  let scroll = 0;
  let scrollTimer: number | undefined;

  const target = (x: number, y: number) => {
    overLater = t.laterTab.contains(document.elementFromPoint(x, y));
    t.laterTab.classList.toggle("is-drop-target", overLater);
    slot = null;
    if (!overLater) {
      // The editor under the pointer, or the nearest one vertically.
      let best: { ed: DocEditor; d: number } | null = null;
      for (const ed of t.editors()) {
        const r = ed.view.dom.getBoundingClientRect();
        if (r.height === 0) continue;
        const d = y < r.top ? r.top - y : y > r.bottom ? y - r.bottom : 0;
        if (!best || d < best.d) best = { ed, d };
      }
      if (best && best.d < 120) slot = slotIn(best.ed, y);
    }
    if (slot) {
      marker.style.transform = `translate(${slot.left + window.scrollX}px, ${slot.y + window.scrollY - 1}px)`;
      marker.style.width = `${slot.right - slot.left}px`;
      marker.classList.add("is-shown");
    } else {
      marker.classList.remove("is-shown");
    }
  };

  const stop = () => {
    dragging = null;
    slot = null;
    overLater = false;
    window.clearInterval(scrollTimer);
    scrollTimer = undefined;
    marker.classList.remove("is-shown");
    t.laterTab.classList.remove("is-drop-target");
    document.body.classList.remove("is-dragging");
    hideGrip();
  };

  const drop = () => {
    if (!dragging) return;
    const { editor: src, i, j } = dragging;
    const lines = src.lines();
    const block = lines.slice(i, j + 1);
    if (overLater) {
      t.toLater(block);
      src.setLines(removeBlock(lines, i, j));
    } else if (slot && slot.editor === src) {
      const r = moveWithin(lines, i, j, slot.line);
      if (r) src.setLines(r.lines, r.start);
    } else if (slot) {
      const r = insertBlock(slot.editor.lines(), slot.line, block);
      slot.editor.setLines(r.lines, r.start);
      src.setLines(removeBlock(lines, i, j));
    }
  };

  grip.addEventListener("pointerdown", (e) => {
    if (!hovered || e.button !== 0) return;
    e.preventDefault();
    const lines = hovered.editor.lines();
    dragging = { editor: hovered.editor, i: hovered.line, j: blockEnd(lines, hovered.line) };
    grip.setPointerCapture(e.pointerId);
    document.body.classList.add("is-dragging");
  });

  grip.addEventListener("pointermove", (e) => {
    if (!dragging) return;
    grip.style.transform = `translate(${e.clientX + window.scrollX - 5}px, ${e.clientY + window.scrollY - 11}px)`;
    scroll = e.clientY < EDGE ? -12 : e.clientY > window.innerHeight - EDGE ? 12 : 0;
    if (scroll && scrollTimer === undefined) {
      scrollTimer = window.setInterval(() => {
        window.scrollBy(0, scroll);
        if (!scroll) {
          window.clearInterval(scrollTimer);
          scrollTimer = undefined;
        }
      }, 16);
    }
    target(e.clientX, e.clientY);
  });

  grip.addEventListener("pointerup", () => {
    drop();
    stop();
  });

  window.addEventListener("keydown", (e) => {
    if (dragging && e.key === "Escape") {
      e.preventDefault();
      stop();
    }
  });
}

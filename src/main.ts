import "@fontsource/source-serif-4/400.css";
import "@fontsource/source-serif-4/400-italic.css";
import "@fontsource/source-serif-4/600.css";
import "@fontsource/ibm-plex-mono/400.css";
import "@fontsource/ibm-plex-mono/500.css";
import "./tokens.css";
import "./style.css";
import { wireThemeSwitch } from "./theme";

import { api, type Hit, type SyncStatus } from "./api";
import { addDays, meta, title, today } from "./dates";
import { insertBlock } from "./editor/blocks";
import { createEditor, type DocEditor } from "./editor/editor";
import { enableDrag } from "./drag";

type View = "today" | "later" | "days" | "search";

const $ = <T extends HTMLElement>(sel: string) => document.querySelector<T>(sel)!;
const DAY_SEED = "- [ ] ";

// ---------------------------------------------------------------- the stream
// Tomorrow at the top, then today, then every earlier day that has a file.

let stream: DocEditor[] = [];
let streamDate = "";

async function buildStream() {
  await Promise.all(stream.map((e) => e.flush()));
  stream.forEach((e) => e.view.destroy());
  stream = [];

  const root = $("#view-today");
  root.replaceChildren();
  const t = today();
  streamDate = t;
  const existing = await api.days();
  const dates = [...new Set([addDays(t, 1), t, ...existing])].sort().reverse();
  const contents = await Promise.all(dates.map((d) => api.read(`days/${d}`)));

  dates.forEach((date, i) => {
    const section = document.createElement("article");
    section.className = "day" + (date === t ? " is-today" : date > t ? " is-future" : " is-past");
    section.dataset.date = date;

    const metaEl = document.createElement("div");
    metaEl.className = "day-meta";
    metaEl.textContent = meta(date);
    const heading = document.createElement(date === t ? "h1" : "h2");
    heading.className = "day-title";
    heading.textContent = title(date);
    const parent = document.createElement("div");
    parent.className = "doc";
    section.append(metaEl, heading, parent);
    root.append(section);

    const index = i;
    stream.push(
      createEditor({
        parent,
        name: `days/${date}`,
        doc: contents[i],
        seed: DAY_SEED,
        placeholder: date === t ? "What matters today?" : date > t ? "Plan ahead…" : "Nothing written",
        onLeave: (dir) => {
          const next = stream[index + dir];
          if (!next) return false;
          next.focusEdge(dir < 0 ? "end" : "start");
          return true;
        },
        // Alt+Up past the first item goes to the end of the day above,
        // Alt+Down past the last to the start of the day below.
        onMoveOut: (dir, block) => {
          const next = stream[index + dir];
          if (!next) return false;
          const lines = next.lines();
          const r = insertBlock(lines, dir < 0 ? lines.length : 0, block);
          next.setLines(r.lines, r.start);
          next.view.focus();
          return true;
        },
      }),
    );
  });
}

function scrollToDay(date: string, line?: number) {
  show("today");
  const i = stream.findIndex((e) => e.name === `days/${date}`);
  if (i < 0) return;
  const el = $("#view-today").children[i] as HTMLElement;
  el.scrollIntoView({ block: "start" });
  if (line !== undefined) stream[i].focusLine(line);
}

// ---------------------------------------------------------------- later

let later: DocEditor;

async function buildLater() {
  later = createEditor({
    parent: $("#later-editor"),
    name: "later",
    doc: await api.read("later"),
    seed: "- ",
    placeholder: "Anything you want to come back to",
  });
}

// ---------------------------------------------------------------- days list

async function renderDays() {
  const list = $("#days-list");
  list.replaceChildren();
  const days = await api.days();
  if (!days.length) {
    const li = document.createElement("li");
    li.className = "muted";
    li.textContent = "No days written yet.";
    list.append(li);
  }
  for (const d of days) {
    const li = document.createElement("li");
    const b = document.createElement("button");
    b.type = "button";
    b.innerHTML = `<span class="mono"></span><span></span>`;
    b.children[0].textContent = d;
    b.children[1].textContent = title(d);
    b.onclick = () => scrollToDay(d);
    li.append(b);
    list.append(li);
  }
}

// ---------------------------------------------------------------- search

let searchSeq = 0;

/** Strip list/heading markers and inline syntax for a readable snippet. */
function snippet(text: string) {
  return text
    .replace(/^\s*(?:[-*+] (?:\[[ xX]\] )?|#{1,6} )/, "")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/(\*\*|__|~~|`|\*|_)/g, "");
}

async function runSearch(query: string) {
  const seq = ++searchSeq;
  await Promise.all([later.flush(), ...stream.map((e) => e.flush())]);
  const hits = await api.search(query);
  if (seq !== searchSeq) return;
  const list = $("#search-results");
  list.replaceChildren();
  if (query.trim() && !hits.length) {
    const li = document.createElement("li");
    li.className = "muted";
    li.textContent = "Nothing found.";
    list.append(li);
  }
  for (const h of hits) list.append(hitItem(h));
}

function hitItem(h: Hit) {
  const li = document.createElement("li");
  const b = document.createElement("button");
  b.type = "button";
  b.innerHTML = `<span class="mono"></span><span></span>`;
  b.children[0].textContent = h.doc === "later" ? "later" : h.doc.slice(5);
  b.children[1].textContent = snippet(h.text);
  b.onclick = () => {
    if (h.doc === "later") {
      show("later");
      later.focusLine(h.line);
    } else {
      scrollToDay(h.doc.slice(5), h.line);
    }
  };
  li.append(b);
  return li;
}

// ---------------------------------------------------------------- changes on disk
// Sync, quick capture or another editor changed a file.

let rebuild: number | undefined;

function onDocChanged({ name, content }: { name: string; content: string }) {
  if (name === "later") return later.remote(content);
  const editor = stream.find((e) => e.name === name);
  if (editor) return editor.remote(content);
  // A day we don't show yet (written on another device): rebuild the stream.
  if (content.trim()) {
    window.clearTimeout(rebuild);
    rebuild = window.setTimeout(() => void buildStream(), 300);
  }
}

// ---------------------------------------------------------------- sync status
// Always a dot; words only when something needs attention.

function clock(unix: number) {
  return new Date(unix * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

// A sync often takes milliseconds: hold the pulse long enough to be seen.
const MIN_PULSE = 700;
let pulseSince = 0;
let held: number | undefined;

function showSync(s: SyncStatus) {
  window.clearTimeout(held);
  const shown = $("#sync").dataset.state;
  if (s.state === "syncing" && shown !== "syncing") pulseSince = Date.now();
  const wait = pulseSince + MIN_PULSE - Date.now();
  if (shown === "syncing" && s.state !== "syncing" && wait > 0) {
    held = window.setTimeout(() => renderSync(s), wait);
    return;
  }
  renderSync(s);
}

function renderSync(s: SyncStatus) {
  const el = $("#sync");
  el.dataset.state = s.state;
  const waiting = s.pending ? ` · ${s.pending} waiting` : "";
  const text: Record<SyncStatus["state"], string> = {
    off: "",
    syncing: "",
    synced: "",
    offline: `offline${waiting}`,
    error: `sync error${waiting}`,
  };
  const tip: Record<SyncStatus["state"], string> = {
    off: "Sync is not set up (see the README).",
    syncing: "Syncing…",
    synced: s.last ? `Synced at ${clock(s.last)}.` : "Synced.",
    offline: `Can't reach the sync server${s.last ? `; last synced ${clock(s.last)}` : ""}. Changes are kept and sent later.`,
    error: s.message ?? "Sync failed.",
  };
  $("#sync-text").textContent = text[s.state];
  el.title = `${tip[s.state]}${s.state === "off" ? "" : " Click to sync now."}`;
  el.setAttribute("aria-label", tip[s.state]);
}

// ---------------------------------------------------------------- views

const views: View[] = ["today", "later", "days", "search"];

function show(v: View) {
  for (const name of views) {
    $(`#view-${name}`).hidden = name !== v;
    const tab = $(`.tabs [data-view="${name}"]`);
    if (name === v) tab.setAttribute("aria-current", "page");
    else tab.removeAttribute("aria-current");
  }
  if (v === "days") void renderDays();
  if (v === "search") $<HTMLInputElement>("#search-input").focus();
  if (v === "later" && !later.view.hasFocus) later.focusEdge("end");
}

function wire() {
  wireThemeSwitch($<HTMLButtonElement>("#theme-switch"));
  enableDrag({
    editors: () => ($("#view-later").hidden ? stream : [later]),
    toLater: (block) => {
      const lines = later.lines();
      later.setLines(insertBlock(lines, lines.length, block).lines);
    },
    laterTab: $(`.tabs [data-view="later"]`),
  });
  $<HTMLButtonElement>("#sync").onclick = () => {
    void api.syncNow();
    window.setTimeout(() => void api.syncStatus().then(showSync), 1500);
  };
  void api.onDocChanged(onDocChanged);
  void api.onSyncStatus(showSync);
  void api.syncStatus().then(showSync);

  document.querySelectorAll<HTMLButtonElement>(".tabs button").forEach((b) => {
    b.onclick = () => show(b.dataset.view as View);
  });

  let debounce: number | undefined;
  $<HTMLInputElement>("#search-input").addEventListener("input", (e) => {
    window.clearTimeout(debounce);
    const q = (e.target as HTMLInputElement).value;
    debounce = window.setTimeout(() => void runSearch(q), 120);
  });

  // Mod-1..4 switch views, Mod-K searches.
  window.addEventListener("keydown", (e) => {
    const mod = navigator.platform.startsWith("Mac") ? e.metaKey : e.ctrlKey;
    if (!mod || e.altKey || e.shiftKey) return;
    const n = Number(e.key);
    if (n >= 1 && n <= views.length) show(views[n - 1]);
    else if (e.key.toLowerCase() === "k") show("search");
    else return;
    e.preventDefault();
  });

  // Save before the window goes away; rebuild after midnight.
  const flushAll = () => {
    void later.flush();
    stream.forEach((e) => void e.flush());
  };
  window.addEventListener("pagehide", flushAll);
  document.addEventListener("visibilitychange", () => {
    if (!document.hidden) void api.syncStatus().then(showSync);
    if (document.hidden) flushAll();
    else if (today() !== streamDate) void buildStream();
  });
  window.addEventListener("focus", () => {
    if (today() !== streamDate) void buildStream();
    void api.syncNow();
    void api.syncStatus().then(showSync);
  });
}

async function start() {
  await Promise.all([buildStream(), buildLater()]);
  wire();
}

void start();

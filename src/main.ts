import "@fontsource/source-serif-4/400.css";
import "@fontsource/source-serif-4/400-italic.css";
import "@fontsource/source-serif-4/600.css";
import "@fontsource/ibm-plex-mono/400.css";
import "@fontsource/ibm-plex-mono/500.css";
import "./tokens.css";
import "./style.css";
import { wireThemeSwitch } from "./theme";

import { api, isMobile, type Hit, type NoteInfo, type SyncStatus } from "./api";
import { addDays, meta, title, today } from "./dates";
import { insertBlock } from "./editor/blocks";
import { createEditor, type DocEditor } from "./editor/editor";
import { enableDrag } from "./drag";
import { mountEditBar } from "./editbar";
import { mountSyncPopover } from "./syncPopover";

type View = "today" | "notes" | "map" | "days" | "search";

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
        newItem: DAY_SEED,
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

// ---------------------------------------------------------------- notes
// The Notes tab is either the list or one open note. Later is a note too,
// always first; its editor lives as long as the app (it is a drop target).

let later: DocEditor;
/** The open note other than Later, if any. */
let note: DocEditor | undefined;
/** What the Notes tab shows: "later", "notes/<slug>" or the list. */
let opened: string | null = null;
let notes: NoteInfo[] = [];
let pins = "";

async function buildLater() {
  later = createEditor({
    parent: $("#later-editor"),
    name: "later",
    doc: await api.read("later"),
    seed: "- ",
    placeholder: "Anything you want to come back to",
  });
}

const pinned = () => new Set(pins.split("\n").map((l) => l.trim()).filter(Boolean));

async function togglePin(name: string) {
  const set = pinned();
  if (!set.delete(name)) set.add(name);
  const next = [...set].map((n) => n + "\n").join("");
  pins = await api.save("pins", next, pins);
  void renderNotes();
}

function noteTitle(name: string) {
  return name === "later" ? "Later" : (notes.find((n) => n.name === name)?.title ?? name.slice(6));
}

async function renderNotes() {
  [notes, pins] = await Promise.all([api.notes(), api.read("pins")]);
  const set = pinned();
  const sorted = [...notes.filter((n) => set.has(n.name)), ...notes.filter((n) => !set.has(n.name))];

  const list = $("#notes-list");
  list.replaceChildren(newNoteItem(), noteItem("later", "Later", "later", null));
  for (const n of sorted) {
    const date = n.modified ? new Date(n.modified * 1000).toLocaleDateString("sv-SE") : "";
    list.append(noteItem(n.name, n.title, date, set.has(n.name)));
  }
}

function noteItem(name: string, text: string, label: string, isPinned: boolean | null) {
  const li = document.createElement("li");
  const b = document.createElement("button");
  b.type = "button";
  b.innerHTML = `<span class="mono"></span><span></span>`;
  b.children[0].textContent = label;
  b.children[1].textContent = text;
  b.onclick = () => void openNote(name);
  li.append(b);
  if (isPinned !== null) {
    const pin = document.createElement("button");
    pin.type = "button";
    pin.className = "pin";
    pin.textContent = isPinned ? "pinned" : "pin";
    pin.setAttribute("aria-pressed", String(isPinned));
    pin.setAttribute("aria-label", `${isPinned ? "Unpin" : "Pin"} ${text}`);
    pin.onclick = () => void togglePin(name);
    li.append(pin);
  }
  return li;
}

/** "+ New note", which turns into a name field. */
function newNoteItem() {
  const li = document.createElement("li");
  const b = document.createElement("button");
  b.type = "button";
  b.className = "new-note";
  b.innerHTML = `<span class="mono">+</span><span>New note</span>`;
  b.onclick = () => {
    const input = document.createElement("input");
    input.className = "new-note-input";
    input.placeholder = "Name of the note";
    input.setAttribute("aria-label", "Name of the new note");
    input.onkeydown = (e) => {
      if (e.key === "Escape") li.replaceWith(newNoteItem());
      if (e.key !== "Enter" || !input.value.trim()) return;
      e.preventDefault();
      void createNote(input.value.trim());
    };
    input.onblur = () => {
      if (!input.value.trim()) li.replaceWith(newNoteItem());
    };
    li.replaceChildren(input);
    input.focus();
  };
  li.append(b);
  return li;
}

/** "Trip ideas!" -> "trip-ideas"; a valid name in core's `is_slug` sense. */
function slugify(title: string) {
  const s = title
    .toLowerCase()
    .replace(/ä/g, "ae")
    .replace(/ö/g, "oe")
    .replace(/ü/g, "ue")
    .replace(/ß/g, "ss")
    .normalize("NFKD")
    .replace(/[̀-ͯ]/g, "")
    .replace(/[^a-z0-9]+/g, "-")
    .slice(0, 60)
    .replace(/^-+|-+$/g, "");
  return s || "note";
}

async function createNote(title: string) {
  const taken = new Set((await api.notes()).map((n) => n.name));
  const slug = slugify(title);
  let name = `notes/${slug}`;
  for (let i = 2; taken.has(name); i++) name = `notes/${slug}-${i}`;
  await api.save(name, `# ${title}\n\n- `, "");
  await openNote(name);
}

async function openNote(name: string, line?: number) {
  if (note && note.name !== name) {
    await note.flush();
    note.view.destroy();
    note = undefined;
  }
  if (name !== "later" && !note) {
    note = createEditor({ parent: $("#note-editor"), name, doc: await api.read(name), seed: "- ", placeholder: "Write anything" });
  }
  opened = name;
  showNotes();
  const editor = name === "later" ? later : note!;
  if (line !== undefined) editor.focusLine(line);
  else if (!editor.view.hasFocus) editor.focusEdge("end");
}

async function closeNote() {
  opened = null;
  if (note) {
    await note.flush();
    note.view.destroy();
    note = undefined;
  }
  showNotes();
}

/** Show the Notes tab's current state: the list or the open note. */
function showNotes() {
  $("#notes-index").hidden = opened !== null;
  $("#note-later").hidden = opened !== "later";
  $("#note-other").hidden = opened === null || opened === "later";
  if (opened === null) void renderNotes();
}

// ---------------------------------------------------------------- days list

async function renderDays() {
  await fillDays($("#days-list"));
}

/** The list of days, in `list`. Also the phone's empty Search. */
async function fillDays(list: HTMLElement, isCurrent: () => boolean = () => true) {
  const days = await api.days();
  if (!isCurrent()) return;
  list.replaceChildren();
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
/** The phone layout: the phone app, or any narrow window (see style.css). */
const compact = window.matchMedia("(max-width: 600px)");

/** Strip list/heading markers and inline syntax for a readable snippet. */
function snippet(text: string) {
  return text
    .replace(/^\s*(?:[-*+] (?:\[[ xX]\] )?|#{1,6} )/, "")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/(\*\*|__|~~|`|\*|_)/g, "");
}

async function runSearch(query: string) {
  const seq = ++searchSeq;
  await flushAll();
  const list = $("#search-results");
  // The phone has no Days tab: an empty search lists the days instead.
  if (!query.trim() && compact.matches) return fillDays(list, () => seq === searchSeq);
  const [hits, all] = await Promise.all([api.search(query), api.notes()]);
  if (seq !== searchSeq) return;
  notes = all;
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
  const isDay = h.doc.startsWith("days/");
  b.children[0].textContent = isDay ? h.doc.slice(5) : noteTitle(h.doc).toLowerCase();
  b.children[1].textContent = snippet(h.text);
  b.onclick = () => {
    if (isDay) return scrollToDay(h.doc.slice(5), h.line);
    show("notes");
    void openNote(h.doc, h.line);
  };
  li.append(b);
  return li;
}

// ---------------------------------------------------------------- changes on disk
// Sync, quick capture or another editor changed a file.

let rebuild: number | undefined;
let relist: number | undefined;
let remap: number | undefined;

function onDocChanged({ name, content }: { name: string; content: string }) {
  // Places live in notes and Later: keep a visible map current.
  if ((name === "later" || name.startsWith("notes/")) && map && current() === "map") {
    window.clearTimeout(remap);
    remap = window.setTimeout(() => void map?.then((m) => m.refresh()), 300);
  }
  if (name === "later") return later.remote(content);
  if (name === "pins" || name.startsWith("notes/")) {
    if (note?.name === name) note.remote(content);
    if (opened === null && !$("#view-notes").hidden) {
      window.clearTimeout(relist);
      relist = window.setTimeout(() => void renderNotes(), 300);
    }
    return;
  }
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

let syncPanel: ReturnType<typeof mountSyncPopover> | undefined;

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
    off: isMobile ? "Connect" : "",
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
  el.title = tip[s.state];
  el.setAttribute("aria-label", tip[s.state]);
  syncPanel?.setStatus(tip[s.state]);
}

// ---------------------------------------------------------------- views

const views: View[] = ["today", "notes", "map", "days", "search"];

// ---------------------------------------------------------------- map
// Loaded on first visit: MapLibre is large and most days don't need it.

let map: Promise<{ refresh(): Promise<void>; resize(): void }> | undefined;

async function showMap() {
  if (!map) {
    map = import("./map/map").then(({ mountMap }) =>
      mountMap($("#map"), {
        openDoc: (name, line) => {
          show("notes");
          void openNote(name, line);
        },
      }),
    );
  }
  try {
    const m = await map;
    m.resize();
    // Notes may have changed while the map was out of sight.
    await flushAll();
    await m.refresh();
  } catch (e) {
    // Say why instead of an empty page (no WebGL, a broken style...); the
    // next visit tries again.
    map = undefined;
    const p = document.createElement("p");
    p.className = "muted";
    p.textContent = `The map could not start: ${e instanceof Error ? e.message : String(e)}`;
    $("#map").replaceChildren(p);
  }
}

function current(): View {
  return views.find((v) => !$(`#view-${v}`).hidden) ?? "today";
}

function show(v: View) {
  // The Notes tab again, while on a note: back to the list.
  if (v === "notes" && current() === "notes" && opened !== null) void closeNote();
  for (const name of views) {
    $(`#view-${name}`).hidden = name !== v;
    const tab = $(`.tabs [data-view="${name}"]`);
    if (name === v) tab.setAttribute("aria-current", "page");
    else tab.removeAttribute("aria-current");
  }
  if (v === "days") void renderDays();
  if (v === "search") {
    const input = $<HTMLInputElement>("#search-input");
    input.focus();
    if (!input.value) void runSearch("");
  }
  if (v === "notes") showNotes();
  if (v === "map") void showMap();
}

/** Write every editor's pending changes. */
function flushAll() {
  return Promise.all([later.flush(), note?.flush(), ...stream.map((e) => e.flush())]);
}

function wire() {
  wireThemeSwitch($<HTMLButtonElement>("#theme-switch"));

  // The interface never zooms: pinch and Ctrl+scroll are for the map, which
  // handles them first on its own element.
  window.addEventListener("wheel", (e) => e.ctrlKey && e.preventDefault(), { passive: false });
  window.addEventListener("touchmove", (e) => e.touches.length > 1 && e.preventDefault(), { passive: false });
  for (const g of ["gesturestart", "gesturechange", "gestureend"]) {
    window.addEventListener(g, (e) => e.preventDefault(), { passive: false });
  }
  enableDrag({
    editors: () => {
      if (current() === "today") return stream;
      if (current() !== "notes") return [];
      return opened === "later" ? [later] : note ? [note] : [];
    },
    // Dropped on the Notes tab: into Later.
    toLater: (block) => {
      const lines = later.lines();
      later.setLines(insertBlock(lines, lines.length, block).lines);
    },
    laterTab: $(`.tabs [data-view="notes"]`),
  });
  document.querySelectorAll<HTMLButtonElement>("#view-notes .back").forEach((b) => {
    b.onclick = () => void closeNote();
  });
  syncPanel = mountSyncPopover({
    button: $<HTMLButtonElement>("#sync"),
    syncNow: () => {
      void api.syncNow();
      window.setTimeout(() => void api.syncStatus().then(showSync), 1500);
    },
  });
  if (isMobile) mountEditBar(() => [later, note, ...stream].find((e) => e?.view.hasFocus));
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
  window.addEventListener("pagehide", () => void flushAll());
  document.addEventListener("visibilitychange", () => {
    if (!document.hidden) void api.syncStatus().then(showSync);
    if (document.hidden) void flushAll();
    else if (today() !== streamDate) void buildStream();
  });
  window.addEventListener("focus", () => {
    if (today() !== streamDate) void buildStream();
    void api.syncNow();
    void api.syncStatus().then(showSync);
  });
}

/** `html.compact` is the phone layout (CSS), `html.is-mobile` the phone app. */
function markLayout() {
  const root = document.documentElement;
  root.classList.toggle("is-mobile", isMobile);
  const set = () => root.classList.toggle("compact", isMobile || compact.matches);
  set();
  compact.addEventListener("change", set);
}

async function start() {
  markLayout();
  await Promise.all([buildStream(), buildLater()]);
  wire();
}

void start();

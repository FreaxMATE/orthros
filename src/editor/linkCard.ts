// Link cards: a line that is only a URL ("https://..." or "- https://...")
// is drawn as a card (image, title, description, site) until the cursor is
// on the line; then the raw address shows for editing. Links written as
// [text](url) stay inline links (livePreview.ts).
//
// The card is a plain link first (host and path) and fills in when the
// preview arrives, in place, so the widget survives keystrokes elsewhere.
// Map links get the place card (places/placeCard.ts) instead.
// A preview that came back empty (offline) is asked for again after a while.

import { RangeSetBuilder } from "@codemirror/state";
import { Decoration, EditorView, ViewPlugin, WidgetType, type DecorationSet, type ViewUpdate } from "@codemirror/view";
import { api, openUrl, type LinkPreview, type Place, type PlaceDetails } from "../api";
import { withVisited } from "../map/places";
import { placeCard } from "../places/placeCard";
import { fences } from "./code";
import { lineInfo } from "./livePreview";

const BARE_URL = /^https?:\/\/[^\s<>]+$/i;
/** How long an empty answer is believed before we ask again. */
const RETRY_MS = 60_000;

interface Entry {
  promise: Promise<LinkPreview | null>;
  /** When an empty answer arrived (0 while pending or filled). */
  empty: number;
}
const cache = new Map<string, Entry>();

/** Map links: their pages all say "Google Maps: find local businesses...",
 *  so the card shows the place itself (what the map tab pins) instead. */
const MAP_URL = /^https?:\/\/(?:(?:www\.)?google\.[a-z.]+\/maps|maps\.google\.[a-z.]+\/|maps\.app\.goo\.gl\/|goo\.gl\/maps|(?:www\.)?(?:openstreetmap|osm)\.org\/|maps\.apple\.com\/)/i;

/** What a place card needs; details stay null while the app has none. */
interface PlaceData {
  place: Place;
  details: PlaceDetails | null;
}
interface PlaceEntry {
  promise: Promise<PlaceData | null>;
  empty: number;
}
const places = new Map<string, PlaceEntry>();

async function placeData(url: string): Promise<PlaceData | null> {
  const [place] = await api.resolvePlaces([url]);
  if (!place) return null;
  const details = await api.placeDetails(place.lat, place.lng, place.name).catch(() => null);
  return { place, details };
}

/** Like preview(): cached by URL; no place, or a place without details yet
 *  (offline), is asked for again after RETRY_MS. */
function placeFor(url: string): Promise<PlaceData | null> {
  const hit = places.get(url);
  if (hit && !(hit.empty && Date.now() - hit.empty > RETRY_MS)) return hit.promise;
  const entry: PlaceEntry = { empty: 0, promise: Promise.resolve(null) };
  entry.promise = placeData(url)
    .catch(() => null)
    .then((d) => {
      if (!d?.details) entry.empty = Date.now();
      return d;
    });
  places.set(url, entry);
  return entry.promise;
}

function preview(url: string): Promise<LinkPreview | null> {
  const hit = cache.get(url);
  if (hit && !(hit.empty && Date.now() - hit.empty > RETRY_MS)) return hit.promise;
  const entry: Entry = { empty: 0, promise: Promise.resolve(null) };
  entry.promise = api.linkPreview(url)
    .catch(() => null)
    .then((p) => {
      if (!p) entry.empty = Date.now();
      return p;
    });
  cache.set(url, entry);
  return entry.promise;
}

function parts(url: string): { host: string; plain: string } {
  try {
    const u = new URL(url);
    const host = u.hostname.replace(/^www\./, "");
    const path = (u.pathname + u.search).replace(/\/$/, "");
    return { host, plain: host + path };
  } catch {
    return { host: url, plain: url };
  }
}

function span(cls: string, text: string) {
  const el = document.createElement("span");
  el.className = cls;
  el.textContent = text;
  return el;
}

class CardWidget extends WidgetType {
  /** `visited`: the line is ticked (a place you have been to). */
  constructor(readonly url: string, readonly visited: boolean) {
    super();
  }
  eq(o: CardWidget) {
    return o.url === this.url && o.visited === this.visited;
  }
  toDOM(view: EditorView) {
    if (MAP_URL.test(this.url)) return this.placeDOM(view);
    const { host, plain } = parts(this.url);
    const a = document.createElement("a");
    a.className = "cm-card is-plain";
    a.href = this.url;
    a.rel = "noopener noreferrer";
    a.dataset.href = this.url;
    a.textContent = plain;
    a.addEventListener("click", (e) => {
      e.preventDefault();
      void openUrl(this.url);
    });
    void preview(this.url).then((p) => {
      if (!p) return;
      a.classList.remove("is-plain");
      a.replaceChildren();
      // Only the site (it hides behind a bot check): its icon and name.
      if (!p.title && !p.description) {
        a.classList.add("is-site");
        if (p.image) {
          const icon = document.createElement("img");
          icon.className = "cm-card-icon";
          icon.alt = "";
          icon.src = p.image;
          icon.addEventListener("error", () => icon.remove());
          a.appendChild(icon);
        }
        a.appendChild(span("cm-card-title", p.site || host));
        return;
      }
      if (p.image) {
        const img = document.createElement("img");
        img.className = "cm-card-img";
        img.alt = "";
        img.src = p.image;
        // A broken image just goes away; the card keeps its text.
        img.addEventListener("error", () => img.remove());
        a.appendChild(img);
      }
      const body = document.createElement("span");
      body.className = "cm-card-body";
      body.append(span("cm-card-title", p.title || host));
      if (p.description) body.append(span("cm-card-desc", p.description));
      body.append(span("cm-card-site", p.site || host));
      a.appendChild(body);
    });
    return a;
  }
  /** A map link: the place card. The card holds several links, so the
   *  widget is a group, not an <a>; until data arrives it is the plain link. */
  placeDOM(view: EditorView) {
    const { plain } = parts(this.url);
    const box = document.createElement("div");
    box.className = "cm-card is-plain";
    box.setAttribute("role", "group");
    const a = document.createElement("a");
    a.href = this.url;
    a.rel = "noopener noreferrer";
    a.textContent = plain;
    a.addEventListener("click", (e) => {
      e.preventDefault();
      void openUrl(this.url);
    });
    box.append(a);
    void placeFor(this.url).then((d) => {
      if (!d) return;
      box.className = "cm-place";
      const set = (visited: boolean) => {
        const line = view.state.doc.lineAt(view.posAtDOM(box));
        view.dispatch({ changes: { from: line.from, to: line.to, insert: withVisited(line.text, visited) } });
      };
      box.replaceChildren(placeCard({ url: this.url, place: d.place, details: d.details, saved: { visited: this.visited, set } }));
    });
    return box;
  }
  // Our own click handler does the work; the editor must not move the cursor.
  ignoreEvent() {
    return true;
  }
}

function build(view: EditorView): DecorationSet {
  const { state } = view;
  const active = new Set<number>();
  if (view.hasFocus) {
    for (const r of state.selection.ranges) {
      const a = state.doc.lineAt(r.from).number;
      const b = state.doc.lineAt(r.to).number;
      for (let n = a; n <= b; n++) active.add(n);
    }
  }
  const out = new RangeSetBuilder<Decoration>();
  for (const { from, to } of view.visibleRanges) {
    const code = fences(state, from, to);
    for (let pos = from; pos <= to; ) {
      const line = state.doc.lineAt(pos);
      pos = line.to + 1;
      if (active.has(line.number) || code.some((f) => line.number >= f.first && line.number <= f.last)) continue;
      const info = lineInfo(line.text);
      if (info.kind !== "bullet" && info.kind !== "text" && info.kind !== "task") continue;
      const url = line.text.slice(info.contentStart).trim();
      if (!BARE_URL.test(url)) continue;
      // On a task line only places get a card: the checkbox means something there.
      if (info.kind === "task" && !MAP_URL.test(url)) continue;
      out.add(line.from + info.contentStart, line.to, Decoration.replace({ widget: new CardWidget(url, info.done) }));
    }
  }
  return out.finish();
}

export const linkCards = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;
    constructor(view: EditorView) {
      this.decorations = build(view);
    }
    update(u: ViewUpdate) {
      if (u.docChanged || u.selectionSet || u.viewportChanged || u.focusChanged) this.decorations = build(u.view);
    }
  },
  { decorations: (v) => v.decorations },
);

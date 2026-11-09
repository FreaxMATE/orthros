// The bridge to the Rust core. Outside Tauri (plain `npm run dev` in a
// browser) it falls back to localStorage so the UI can be worked on alone.

export interface Hit {
  doc: string;
  line: number;
  text: string;
}

export interface NoteInfo {
  /** Document name, "notes/<slug>". */
  name: string;
  title: string;
  /** Unix seconds. */
  modified: number;
}

export interface LinkPreview {
  url: string;
  title: string | null;
  description: string | null;
  site: string | null;
  /** A data: URL, or null. */
  image: string | null;
}

export interface Place {
  lat: number;
  lng: number;
  /** The place's name, if the link carries one. */
  name: string | null;
}

/** What the app could find out about a place (OpenStreetMap, Wikipedia);
 *  every field is null where nothing was found. */
export interface PlaceDetails {
  name: string | null;
  /** "Shinto shrine". */
  kind: string | null;
  address: string | null;
  /** Raw OSM opening_hours, e.g. "Mo-Su 08:30-16:30". */
  opening_hours: string | null;
  website: string | null;
  phone: string | null;
  /** A Wikipedia extract. */
  description: string | null;
  /** The article's URL. */
  wikipedia: string | null;
  /** A data: URL, or null. */
  photo: string | null;
  /** The OpenStreetMap object's URL. */
  osm: string | null;
}

export interface SyncStatus {
  state: "off" | "syncing" | "synced" | "offline" | "error";
  message: string | null;
  pending: number;
  last: number | null;
}

export interface SyncConfig {
  server: string;
  token: string;
}

export const inTauri = "__TAURI_INTERNALS__" in window;
/** The phone app (`?mobile` fakes it in a browser, for layout work). */
export const isMobile = /Android/i.test(navigator.userAgent) || new URLSearchParams(location.search).has("mobile");

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

async function on<T>(event: string, handler: (payload: T) => void) {
  if (!inTauri) return;
  const { listen } = await import("@tauri-apps/api/event");
  await listen<T>(event, (e) => handler(e.payload));
}

const KEY = "orthros:";
const demo = !inTauri && new URLSearchParams(location.search).has("demo");

// `?demo` in the browser: start from a sample space (UI work, screenshots).
if (demo) {
  const d = (n: number) => {
    const t = new Date();
    t.setDate(t.getDate() + n);
    return t.toLocaleDateString("sv-SE"); // YYYY-MM-DD
  };
  const sample: Record<string, string> = {
    [`days/${d(1)}`]: "- [ ] Prepare the slides for Friday\n- [ ] Call the dentist\n",
    [`days/${d(0)}`]: [
      "- [x] Review yesterday's notes",
      "- [ ] Write the methods section",
      "  - [x] Outline the evaluation",
      "  - [ ] Describe the **data sources**",
      "- [ ] Reply to _Anna_ about the workshop",
      "- [ ] Fix the plotting script in `plots.py`",
      "## Notes",
      "- The [style guide](https://example.org) wants figures as PDF",
      "",
    ].join("\n"),
    [`days/${d(-1)}`]: "- [x] Submit the travel expenses\n- [x] Read the chapter on sampling\n- [ ] Clean up the bibliography\n",
    later: "- A book on deep work\n- Learn the basics of Rust\n- Try a standing desk\n",
    "notes/life-goals": "# Life goals\n\n- Live a year abroad\n- Learn to sail\n",
    "notes/trip-ideas": "# Trip ideas\n\n- Norway in summer\n- https://www.visitportugal.com\n",
    pins: "notes/life-goals\n",
  };
  for (const [k, v] of Object.entries(sample)) localStorage.setItem(KEY + k, v);
}

/** One fixed place for `?demo`, so the place card can be styled in a browser. */
const demoPlace: PlaceDetails = {
  name: "Fushimi Inari Taisha",
  kind: "Shinto shrine",
  address: "68 Fukakusa Yabunouchicho, Fushimi Ward, Kyoto",
  opening_hours: "Mo-Su 08:30-16:30",
  website: "https://inari.jp",
  phone: "+81 75-641-7331",
  description:
    "Fushimi Inari-taisha is the head shrine of the kami Inari, located in Fushimi-ku, Kyoto. The shrine is famous for its thousands of vermilion torii gates, which straddle a network of trails behind its main buildings.",
  wikipedia: "https://en.wikipedia.org/wiki/Fushimi_Inari-taisha",
  photo: null,
  osm: "https://www.openstreetmap.org/way/31305405",
};

const browser = {
  read: async (name: string) => localStorage.getItem(KEY + name) ?? "",
  save: async (name: string, content: string) => {
    if (content.trim()) localStorage.setItem(KEY + name, content);
    else localStorage.removeItem(KEY + name);
    return content;
  },
  days: async () =>
    Object.keys(localStorage)
      .filter((k) => k.startsWith(KEY + "days/"))
      .map((k) => k.slice(KEY.length + 5))
      .sort()
      .reverse(),
  notes: async (): Promise<NoteInfo[]> =>
    Object.keys(localStorage)
      .filter((k) => k.startsWith(KEY + "notes/"))
      .map((k) => {
        const name = k.slice(KEY.length);
        const heading = /^# (.+)$/m.exec(localStorage.getItem(k) ?? "");
        return { name, title: heading ? heading[1].trim() : name.slice(6), modified: 0 };
      }),
  search: async (query: string) => {
    const q = query.trim().toLowerCase();
    if (!q) return [];
    const notes = (await browser.notes()).map((n) => n.name);
    const docs = ["later", ...notes, ...(await browser.days()).map((d) => `days/${d}`)];
    const hits: Hit[] = [];
    for (const doc of docs) {
      (await browser.read(doc)).split("\n").forEach((text, line) => {
        if (text.toLowerCase().includes(q)) hits.push({ doc, line, text });
      });
    }
    return hits;
  },
};

export const api = {
  read: (name: string) => (inTauri ? call<string>("read_doc", { name }) : browser.read(name)),
  /** Save `content`, which was based on `prev`; resolves to what is on disk
   *  now (a merge if the file changed underneath). */
  save: (name: string, content: string, prev: string) =>
    inTauri ? call<string>("save_doc", { name, content, prev }) : browser.save(name, content),
  days: () => (inTauri ? call<string[]>("list_days") : browser.days()),
  search: (query: string) => (inTauri ? call<Hit[]>("search", { query }) : browser.search(query)),
  notes: () => (inTauri ? call<NoteInfo[]>("list_notes") : browser.notes()),
  /** Title, description and image of a web page; null when it can't be
   *  fetched (offline). Cached on disk by the app. */
  linkPreview: (url: string) => (inTauri ? call<LinkPreview | null>("link_preview", { url }) : Promise.resolve(null)),
  /** Coordinates for each URL, null where it is not a place (or can't be
   *  resolved yet, e.g. a short link while offline). Cached by the app. */
  resolvePlaces: (urls: string[]) =>
    inTauri ? call<(Place | null)[]>("resolve_places", { urls }) : Promise.resolve(urls.map(() => null)),
  /** Address, opening hours, description and photo of a place; null when
   *  nothing was found (or offline). Cached by the app. */
  placeDetails: (lat: number, lng: number, name: string | null) =>
    inTauri
      ? call<PlaceDetails | null>("place_details", { lat, lng, name, lang: navigator.language.slice(0, 2) || "en" })
      : Promise.resolve(demo ? demoPlace : null),
  /** A map tile, style or font, through the app's disk cache. */
  fetchCached: async (url: string): Promise<ArrayBuffer> => {
    if (!inTauri) return (await fetch(url)).arrayBuffer();
    // If Tauri ever falls back to its message channel, bytes arrive as a list.
    const r = await call<ArrayBuffer | number[]>("fetch_cached", { url });
    return Array.isArray(r) ? new Uint8Array(r).buffer : r;
  },

  syncNow: () => (inTauri ? call<void>("sync_now") : Promise.resolve()),
  syncStatus: () =>
    inTauri
      ? call<SyncStatus>("sync_status")
      : Promise.resolve<SyncStatus>(
          demo ? { state: "synced", message: null, pending: 0, last: Date.now() / 1000 } : { state: "off", message: null, pending: 0, last: null },
        ),
  /** The saved sync config (for the QR code), or null when sync is not set up. */
  syncInfo: () => (inTauri ? call<SyncConfig | null>("sync_info") : Promise.resolve(null)),
  /** Save a sync config and sync once; rejects with a message on failure. */
  connectSync: (server: string, token: string) => (inTauri ? call<void>("connect_sync", { server, token }) : Promise.resolve()),
  onSyncStatus: (f: (s: SyncStatus) => void) => on<SyncStatus>("sync-status", f),
  /** A touchpad pinch (Linux, where the page itself never sees one). */
  onPinch: (f: (p: { begin: boolean; scale: number; x: number; y: number }) => void) => on("touchpad-pinch", f),
  onDocChanged: (f: (d: { name: string; content: string }) => void) => on("doc-changed", f),

  capture: (text: string, target: "later" | "today", date: string) =>
    inTauri ? call<void>("capture", { text, target, date }) : Promise.resolve(),
  closeCapture: () => (inTauri ? call<void>("close_capture") : Promise.resolve()),
};

export async function openUrl(url: string) {
  if (inTauri) {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(url);
  } else {
    window.open(url, "_blank", "noopener");
  }
}

// The Map tab: MapLibre with bundled country outlines that always work,
// street tiles or satellite imagery on top when online, and the places found
// in notes as pins. Planned or visited comes from the line's checkbox;
// visited countries are shaded (from ticked places and from the note
// "Been there"). See docs/notes-and-map.md.
//
// All map network traffic goes through api.fetchCached (Rust, disk cache) by
// way of a MapLibre protocol: `cached://host/path` stands for
// `https://host/path`, so the webview never needs network access itself.

import {
  addProtocol,
  type LngLat,
  LngLatBounds,
  Map as MapLibreMap,
  Marker,
  NavigationControl,
  Popup,
  setWorkerUrl,
  type StyleSpecification,
} from "maplibre-gl";
import "maplibre-gl/dist/maplibre-gl.css";
// MapLibre 6 looks for its worker next to its own script, and only when that
// was loaded over http(s): neither holds under Vite or tauri://. Without the
// worker, GeoJSON and vector tiles never render (only raster images do).
// The webview won't start a module worker from a tauri:// file either, so the
// worker ships as text and starts from a blob: URL (allowed by the CSP).
import workerCode from "maplibre-gl/dist/maplibre-gl-worker.mjs?raw";
import "./map.css";
import { api, type Place, type PlaceDetails } from "../api";
import { placeCard } from "../places/placeCard";
import { countryAt, findCountry, loadCountries, type Collection, type Country } from "./countries";
import { FLAG, HEART, icon } from "../places/icons";
import { beenThere, extractLinks, setVisited, type Doc } from "./places";

// The bundled outline is not a mode: it is what shows while the street map
// can't be had (offline, never cached), and under everything else.
type Mode = "streets" | "satellite";
const MODES: [Mode, string][] = [
  ["streets", "Streets"],
  ["satellite", "Satellite"],
];

const BEEN = "notes/been-there";
const STYLE_URL = "https://tiles.openfreemap.org/styles/liberty";
const SAT_URL = "https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{z}/{y}/{x}";
const STREETS_ATTR = "OpenFreeMap © OpenMapTiles Data from OpenStreetMap";
const SAT_ATTR = "Source: Esri, Maxar, Earthstar Geographics, and the GIS User Community";
const MODE_KEY = "orthros:map-mode";
/** Half a pin: how close a click must be to count as on it. */
const PIN_REACH = 13;

interface Pin {
  lng: number;
  lat: number;
  doc: string;
  title: string;
  line: number;
  label: string;
  url: string;
  visited: boolean;
}

// ------------------------------------------------------------- the protocol

const via = (url: string) => url.replace(/^https:\/\//, "cached://");
const unvia = (url: string) => url.replace(/^cached:\/\//, "https://");

async function gunzip(buf: ArrayBuffer): Promise<ArrayBuffer> {
  const b = new Uint8Array(buf);
  // Tiles can arrive still compressed when the Rust side did not decode them.
  if (b.length < 2 || b[0] !== 0x1f || b[1] !== 0x8b || typeof DecompressionStream === "undefined") return buf;
  const stream = new Blob([b]).stream().pipeThrough(new DecompressionStream("gzip"));
  return new Response(stream).arrayBuffer();
}

let protocolAdded = false;
function addCachedProtocol() {
  if (protocolAdded) return;
  protocolAdded = true;
  setWorkerUrl(URL.createObjectURL(new Blob([workerCode], { type: "text/javascript" })));
  addProtocol("cached", async (p) => {
    const buf = await gunzip(await api.fetchCached(unvia(p.url)));
    if (p.type === "json" || p.type === "string") {
      const text = new TextDecoder().decode(buf);
      if (p.type === "string") return { data: text };
      const json = JSON.parse(text);
      // A TileJSON names its tile URLs: they must come back through here too.
      if (Array.isArray(json.tiles)) json.tiles = json.tiles.map(via);
      return { data: json };
    }
    return { data: buf };
  });
}

// ------------------------------------------------------------------ colours

interface Colours {
  water: string;
  land: string;
  border: string;
  visitedLand: string;
  planned: string;
  visited: string;
  bg: string;
}

/** MapLibre needs concrete colours, so read the tokens at runtime. */
function colours(): Colours {
  const css = getComputedStyle(document.documentElement);
  const v = (name: string, fallback: string) => css.getPropertyValue(name).trim() || fallback;
  return {
    water: v("--map-water", "#dbe6ec"),
    land: v("--map-land", "#f3f0ea"),
    border: v("--map-border", "#c8c2b6"),
    visitedLand: v("--map-visited-land", "#ef8a80"),
    planned: v("--map-planned", "#188038"),
    visited: v("--map-visited", "#d93025"),
    bg: v("--bg", "#fdfdfc"),
  };
}

// -------------------------------------------------------------------- style

type Layer = StyleSpecification["layers"][number];
type Sources = StyleSpecification["sources"];

let streets: Promise<StyleSpecification> | null = null;

/** The OpenFreeMap style with every URL pointed at the cached protocol. Not
 *  remembered when it fails, so a later try can succeed. */
function streetsStyle(): Promise<StyleSpecification> {
  streets ??= (async () => {
    const style = JSON.parse(new TextDecoder().decode(await api.fetchCached(STYLE_URL))) as StyleSpecification;
    for (const s of Object.values(style.sources) as any[]) {
      if (typeof s.url === "string") s.url = via(s.url);
      if (Array.isArray(s.tiles)) s.tiles = s.tiles.map(via);
      s.attribution = STREETS_ATTR;
    }
    if (typeof style.glyphs === "string") style.glyphs = via(style.glyphs);
    if (typeof style.sprite === "string") style.sprite = via(style.sprite);
    else if (Array.isArray(style.sprite)) style.sprite = style.sprite.map((s) => ({ ...s, url: via(s.url) }));
    return style;
  })();
  streets.catch(() => (streets = null));
  return streets;
}

// Visited countries are a view of the world: they show while at least a whole
// continent fits (Europe needs about zoom 3 in a normal window), then fade out.
const SHADE_FULL = 3;
const SHADE_GONE = 4;
const fadeOut = (opacity: number): any => ["interpolate", ["linear"], ["zoom"], SHADE_FULL, opacity, SHADE_GONE, 0];

function visitedFilter(names: string[]): any {
  return ["in", ["get", "name"], ["literal", names]];
}

// ------------------------------------------------------------------- popups

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string, text?: string): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

function host(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}

// -------------------------------------------------------------------- mount

export async function mountMap(
  parent: HTMLElement,
  o: { openDoc(name: string, line: number): void },
): Promise<{ refresh(): Promise<void>; resize(): void }> {
  addCachedProtocol();

  // --- DOM
  const bar = el("div", "map-bar");
  const status = el("span");
  const note = el("span", "map-note");
  const left = el("span");
  left.append(status, note);
  const modes = el("div", "map-modes");
  const canvas = el("div", "map-canvas");
  bar.append(left, modes);
  parent.append(bar, canvas);

  let mode = loadMode();
  const buttons = new Map<Mode, HTMLButtonElement>();
  for (const [m, label] of MODES) {
    const b = el("button", undefined, label);
    b.type = "button";
    b.onclick = () => void setMode(m);
    buttons.set(m, b);
    modes.append(b);
  }
  const markModes = () => buttons.forEach((b, m) => b.setAttribute("aria-pressed", String(m === mode)));
  markModes();

  // --- data
  let fc: Collection = { type: "FeatureCollection", features: [] };
  try {
    fc = await loadCountries();
  } catch {
    // no outline: pins and tiles still work
  }
  let pins: Pin[] = [];
  let visitedNames: string[] = [];
  /** Visited countries that only a ticked place explains (no been-there line). */
  let viaPlace = new Map<string, string>();
  /** The been-there note's lines, as of the last compute(). */
  let lastBeen: ReturnType<typeof beenThere> = [];

  async function readDocs(): Promise<Doc[]> {
    const notes = await api.notes();
    const names = [{ name: "later", title: "Later" }, ...notes.map((n) => ({ name: n.name, title: n.title }))];
    return Promise.all(
      names.map(async (n) => ({ ...n, text: await api.read(n.name).catch(() => "") })),
    );
  }

  async function compute() {
    const docs = await readDocs();
    const links = extractLinks(docs);
    const urls = [...new Set(links.map((l) => l.url))];
    const places = await api.resolvePlaces(urls).catch(() => urls.map(() => null));
    const byUrl = new Map(urls.map((u, i) => [u, places[i]]));
    pins = [];
    viaPlace = new Map();
    const visited = new Set<string>();
    for (const l of links) {
      const p = byUrl.get(l.url);
      if (!p) continue;
      pins.push({
        lng: p.lng,
        lat: p.lat,
        doc: l.doc,
        title: l.title,
        line: l.line,
        label: l.label ?? p.name ?? host(l.url),
        url: l.url,
        visited: l.visited,
      });
      if (l.visited) {
        const c = countryAt(p.lng, p.lat);
        if (c) {
          visited.add(c.name);
          if (!viaPlace.has(c.name)) viaPlace.set(c.name, l.title);
        }
      }
    }
    const been = docs.find((d) => d.name === BEEN);
    lastBeen = beenThere(been?.text ?? "");
    for (const e of lastBeen) {
      const c = e.visited ? findCountry(e.text) : null;
      if (c) {
        visited.add(c.name);
        viaPlace.delete(c.name);
      }
    }
    visitedNames = [...visited];
    status.textContent = `${visited.size} ${visited.size === 1 ? "country" : "countries"} · ${pins.length} ${pins.length === 1 ? "place" : "places"}`;
  }

  // --- style per mode
  async function buildStyle(m: Mode): Promise<StyleSpecification> {
    const c = colours();
    const sources: Sources = {
      countries: { type: "geojson", data: fc as any, attribution: "Natural Earth" },
    };
    const base: Layer[] = [
      { id: "base-bg", type: "background", paint: { "background-color": c.water } },
      { id: "base-land", type: "fill", source: "countries", paint: { "fill-color": c.land } },
      { id: "base-border", type: "line", source: "countries", paint: { "line-color": c.border, "line-width": 0.6 } },
    ];
    const middle: Layer[] = [];
    const extra: Partial<StyleSpecification> = {};
    note.textContent = "";
    const s = await streetsStyle().catch(() => null);
    const outline = !s;
    if (m === "satellite") {
      sources.satellite = { type: "raster", tiles: [via(SAT_URL)], tileSize: 256, maxzoom: 19, attribution: SAT_ATTR };
      middle.push({ id: "satellite", type: "raster", source: "satellite" });
    }
    if (s) {
      Object.assign(sources, s.sources);
      extra.glyphs = s.glyphs;
      extra.sprite = s.sprite;
      // Streets: everything but its own background (that would hide the
      // outline where tiles are missing). Satellite: like Google's hybrid
      // view, only roads, borders and names over the imagery.
      const keep = (l: Layer) =>
        m === "streets"
          ? l.type !== "background"
          : l.type === "symbol" || (l.type === "line" && ["transportation", "boundary"].includes((l as any)["source-layer"]));
      middle.push(...s.layers.filter(keep));
    } else {
      note.textContent = "Offline: showing country outlines.";
    }
    const shading: Layer[] = [
      {
        id: "visited-fill",
        type: "fill",
        source: "countries",
        filter: visitedFilter(visitedNames),
        maxzoom: SHADE_GONE,
        paint: { "fill-color": c.visitedLand, "fill-opacity": fadeOut(outline ? 0.85 : 0.55) },
      },
      {
        id: "visited-line",
        type: "line",
        source: "countries",
        filter: visitedFilter(visitedNames),
        maxzoom: SHADE_GONE,
        paint: { "line-color": c.visited, "line-width": 1.5, "line-opacity": fadeOut(1) },
      },
    ];
    // Shading goes under the first road or label, so names stay readable.
    const cut = middle.findIndex((l) => l.type === "symbol" || (l as any)["source-layer"] === "transportation");
    const below = cut < 0 ? middle : middle.slice(0, cut);
    const above = cut < 0 ? [] : middle.slice(cut);
    const layers = [...base, ...below, ...shading, ...above];
    return { version: 8, sources, layers, ...extra } as StyleSpecification;
  }

  // --- the map
  const bounds = new LngLatBounds();
  await compute();
  pins.forEach((p) => bounds.extend([p.lng, p.lat]));
  const hasPins = pins.length > 0;
  // A hidden parent has no size yet: fit again once resize() sees one.
  let pendingFit = hasPins && canvas.clientWidth === 0;

  const map = new MapLibreMap({
    container: canvas,
    style: await buildStyle(mode),
    center: [10, 25],
    zoom: 1,
    ...(hasPins ? { bounds, fitBoundsOptions: { padding: 60, maxZoom: 6 } } : {}),
    attributionControl: { compact: true },
  });
  map.addControl(new NavigationControl({ showCompass: false }), "top-right");
  // A touchpad pinch over the map zooms the map, around the fingers.
  let pinch: { zoom: number; around: LngLat } | null = null;
  void api.onPinch(({ begin, scale, x, y }) => {
    const box = canvas.getBoundingClientRect();
    if (begin) {
      const over = box.width > 0 && x >= box.left && x <= box.right && y >= box.top && y <= box.bottom;
      pinch = over ? { zoom: map.getZoom(), around: map.unproject([x - box.left, y - box.top]) } : null;
      if (pinch) map.stop();
    } else if (pinch && scale > 0) {
      map.easeTo({ zoom: pinch.zoom + Math.log2(scale), around: pinch.around, duration: 0 });
    }
  });
  // Pins are page elements, not a map layer: every pin shows at every zoom
  // (no tiles, no label placement), and they survive a change of style.
  let markers: Marker[] = [];
  function drawPins() {
    markers.forEach((m) => m.remove());
    // Hearts last, so they lie on top of a flag at the same spot.
    markers = [...pins]
      .sort((a, b) => Number(a.visited) - Number(b.visited))
      .map((p) => {
        const element = el("div", `map-pin ${p.visited ? "is-been" : "is-want"}`);
        element.append(icon(p.visited ? HEART : FLAG));
        return new Marker({ element }).setLngLat([p.lng, p.lat]).addTo(map);
      });
  }
  drawPins();

  let switching = 0;
  async function setMode(m: Mode) {
    mode = m;
    try {
      localStorage.setItem(MODE_KEY, m);
    } catch {
      // not remembered
    }
    markModes();
    const mine = ++switching;
    const style = await buildStyle(m);
    // diff:false: the layer lists differ a lot between modes, rebuild cleanly.
    if (mine === switching) map.setStyle(style, { diff: false });
  }

  /** Push fresh data into the live sources; a style swap picks it up itself. */
  function pushData() {
    drawPins();
    try {
      for (const layer of ["visited-fill", "visited-line"]) {
        if (map.getLayer(layer)) map.setFilter(layer, visitedFilter(visitedNames));
      }
    } catch {
      // style mid-swap: buildStyle reads the current state anyway
    }
  }

  function repaint() {
    const c = colours();
    const set = (layer: string, prop: string, value: unknown) => map.getLayer(layer) && map.setPaintProperty(layer, prop as any, value);
    try {
      set("base-bg", "background-color", c.water);
      set("base-land", "fill-color", c.land);
      set("base-border", "line-color", c.border);
      set("visited-fill", "fill-color", c.visitedLand);
      set("visited-line", "line-color", c.visited);
    } catch {
      // style mid-swap: it was built with the new colours or will be repainted
    }
  }
  new MutationObserver(repaint).observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
  window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", repaint);

  // --- interaction
  let popup: Popup | null = null;
  const show = (lngLat: [number, number], content: HTMLElement) => {
    popup?.remove();
    popup = new Popup({ closeButton: true, maxWidth: "300px" }).setLngLat(lngLat).setDOMContent(content).addTo(map);
  };

  map.on("click", (e) => {
    const { x, y } = e.point;
    const hits = pins.filter((p) => {
      const at = map.project([p.lng, p.lat]);
      return Math.abs(at.x - x) <= PIN_REACH && Math.abs(at.y - y) <= PIN_REACH;
    });
    if (hits.length) return pinPopup(hits);
    const c = countryAt(e.lngLat.lng, e.lngLat.lat);
    if (c) countryPopup(c, [e.lngLat.lng, e.lngLat.lat]);
  });

  function pinPopup(hits: Pin[]) {
    const box = el("div", "map-pop");
    const seen = new Set<string>();
    let at: [number, number] = [0, 0];
    for (const p of hits) {
      const key = `${p.doc}:${p.line}:${p.label}`;
      if (seen.has(key) || seen.size >= 5) continue;
      seen.add(key);
      const pos: [number, number] = [p.lng, p.lat];
      if (seen.size === 1) at = pos;
      const item = el("div", "map-pop-item");
      const open = el("button", undefined, "Open note");
      open.type = "button";
      open.onclick = () => {
        popup?.remove();
        o.openDoc(p.doc, Number(p.line));
      };
      // The card shows at once from what the pin knows; the details replace
      // it when they arrive (the popup may be gone by then: harmless).
      const place: Place = { lng: pos[0], lat: pos[1], name: p.label };
      let visited = p.visited;
      let details: PlaceDetails | null = null;
      const draw = () =>
        placeCard({
          url: p.url,
          place,
          details,
          compact: true,
          saved: {
            visited,
            set: (v) => {
              visited = v;
              redraw();
              void saveVisited(p.doc, Number(p.line), p.url, v);
            },
          },
        });
      let card = draw();
      const redraw = () => {
        const next = draw();
        card.replaceWith(next);
        card = next;
      };
      void api
        .placeDetails(place.lat, place.lng, place.name)
        .then((d) => {
          if (!d) return;
          details = d;
          redraw();
        })
        .catch(() => {});
      item.append(card, el("div", "map-pop-sub", p.title), open);
      box.append(item);
    }
    show(at, box);
  }

  function countryPopup(c: Country, at: [number, number]) {
    const box = el("div", "map-pop");
    box.append(el("div", "map-pop-title", c.name));
    if (c.de && c.de !== c.name) box.append(el("div", "map-pop-sub", c.de));
    const entry = ticked(c);
    const fromPlace = viaPlace.get(c.name);
    if (!entry.length && fromPlace) {
      box.append(el("div", "map-pop-sub", `Visited (from a place in ${fromPlace})`));
    } else {
      const b = el("button", undefined, entry.length ? "Remove from visited" : "Mark as visited");
      b.type = "button";
      b.onclick = async () => {
        b.disabled = true;
        popup?.remove();
        await (entry.length ? unmark(c) : mark(c)).catch(() => {});
        await refresh();
      };
      box.append(b);
    }
    show(at, box);
  }

  /** Heart or flag from a pin's card: sets the checkbox on the place's line. */
  async function saveVisited(doc: string, line: number, url: string, visited: boolean) {
    const prev = await api.read(doc);
    const next = setVisited(prev, line, url, visited);
    if (next !== null && next !== prev) await api.save(doc, next, prev);
    await refresh();
  }

  // --- editing the been-there note
  const readBeen = () => api.read(BEEN).catch(() => "");
  const ticked = (c: Country) => lastBeen.filter((e) => e.visited && findCountry(e.text)?.name === c.name);

  async function mark(c: Country) {
    const prev = await readBeen();
    const lines = prev.split("\n");
    // A planned line for this country is ticked rather than duplicated.
    const i = beenThere(prev).find((e) => !e.visited && findCountry(e.text)?.name === c.name)?.line ?? -1;
    let next: string;
    if (i >= 0) {
      lines[i] = lines[i].replace(/\[\s?\]/, "[x]");
      next = lines.join("\n");
    } else {
      const base = prev.trim() ? (prev.endsWith("\n") ? prev : prev + "\n") : "# Been there\n\n";
      next = `${base}- [x] ${c.name}\n`;
    }
    await api.save(BEEN, next, prev);
  }

  async function unmark(c: Country) {
    const prev = await readBeen();
    const drop = new Set(
      beenThere(prev)
        .filter((e) => e.visited && findCountry(e.text)?.name === c.name)
        .map((e) => e.line),
    );
    await api.save(BEEN, prev.split("\n").filter((_l, n) => !drop.has(n)).join("\n"), prev);
  }

  // --- refresh and resize
  async function refresh() {
    await compute();
    pushData();
  }
  return {
    refresh,
    resize() {
      map.resize();
      if (pendingFit && canvas.clientWidth > 0) {
        pendingFit = false;
        map.fitBounds(bounds, { padding: 60, maxZoom: 6, animate: false });
      }
    },
  };
}

function loadMode(): Mode {
  try {
    const m = localStorage.getItem(MODE_KEY);
    if (m === "streets" || m === "satellite") return m;
  } catch {
    // storage unavailable
  }
  return "streets";
}

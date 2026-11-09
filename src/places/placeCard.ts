// The place card: what a map link shows, in the editor and in the map popup.
// Photo, name and kind, whether it is open right now, address, website,
// phone, a clamped Wikipedia extract and links out. Built from plain DOM like
// the rest of the app; without details (offline, unknown place) it falls back
// to name, coordinates and the link.
//
// Every link leaves the webview through openUrl (the app must never navigate
// away) and stops the event, so neither CodeMirror nor MapLibre reacts to a
// click inside the card.

import { openUrl, type Place, type PlaceDetails } from "../api";
import { FLAG, HEART, icon } from "./icons";

export interface PlaceCardOptions {
  /** The map link the place came from. */
  url: string;
  place: Place;
  details: PlaceDetails | null;
  /** The map popup: narrow, photo always on top. */
  compact?: boolean;
  /** Been there or want to go, and how to change it. */
  saved?: { visited: boolean; set(visited: boolean): void };
}

export function coords(lat: number, lng: number) {
  const f = (v: number, pos: string, neg: string) => `${Math.abs(v).toFixed(2)}° ${v < 0 ? neg : pos}`;
  return `${f(lat, "N", "S")}, ${f(lng, "E", "W")}`;
}

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls: string, text?: string): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

function link(href: string, text: string, cls = "place-link"): HTMLAnchorElement {
  const a = el("a", cls, text);
  a.href = href;
  a.rel = "noopener noreferrer";
  a.addEventListener("click", (e) => {
    e.preventDefault();
    e.stopPropagation();
    void openUrl(href).catch(() => {});
  });
  return a;
}

/** The host of a web address, for display; null when it is not http(s) (so a
 *  "javascript:" value from the data never becomes a link). */
function webHost(url: string): string | null {
  try {
    const u = new URL(url);
    return /^https?:$/.test(u.protocol) ? u.hostname.replace(/^www\./, "") : null;
  } catch {
    return null;
  }
}

function row(label: string, ...content: (Node | string)[]) {
  const r = el("div", "place-row");
  r.append(el("span", "place-label", label), ...content);
  return r;
}

function mapsLabel(url: string) {
  if (/openstreetmap|osm\.org/i.test(url)) return "Open in OpenStreetMap";
  if (/apple/i.test(url)) return "Open in Apple Maps";
  return "Open in Google Maps";
}

// ------------------------------------------------------------ opening hours

/** "09:00" today, "Mo 09:00" on another day. */
function when(d: Date, now: Date) {
  const time = d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false });
  if (d.toDateString() === now.toDateString()) return time;
  return `${d.toLocaleDateString([], { weekday: "short" })} ${time}`;
}

/** The status line for a raw OSM value, plus the prettified rules. The
 *  library is big (and LGPL, so it stays a separate chunk): it loads only
 *  when a card with hours is drawn. Throws on a value it can't parse. */
async function hoursStatus(value: string, place: Place) {
  const { default: OpeningHours } = await import("opening_hours");
  // The library wants a nominatim-like object for sunrise/holiday rules; we
  // know the position but not the country, which only matters for holidays.
  const oh = new OpeningHours(value, { lat: place.lat, lon: place.lng, address: { country_code: "", state: "" } });
  const now = new Date();
  const next = oh.getNextChange(now);
  let text: string;
  let state: "open" | "closed" | "unknown";
  if (oh.getUnknown(now)) {
    state = "unknown";
    text = oh.getComment(now) || "Hours vary";
  } else if (oh.getState(now)) {
    state = "open";
    text = next ? `Open now · closes ${when(next, now)}` : "Open now";
  } else {
    state = "closed";
    text = next ? `Closed · opens ${when(next, now)}` : "Closed";
  }
  const rules = oh.prettifyValue().split(";").map((r) => r.trim()).filter(Boolean);
  return { state, text, rules };
}

/** The hours block: raw text first (also the final answer when the value
 *  can't be parsed), replaced by the status line and a week toggle. */
function hoursBlock(value: string, place: Place) {
  const box = el("div", "place-hours");
  box.append(el("span", "place-hours-raw", value));
  void hoursStatus(value, place)
    .then(({ state, text, rules }) => {
      const head = el("button", "place-hours-toggle");
      head.type = "button";
      head.setAttribute("aria-expanded", "false");
      head.append(el("span", `place-status is-${state}`, text), el("span", "place-caret", "▾"));
      const list = el("ul", "place-week");
      list.hidden = true;
      for (const r of rules) list.append(el("li", "", r));
      head.addEventListener("click", (e) => {
        e.preventDefault();
        e.stopPropagation();
        list.hidden = !list.hidden;
        head.setAttribute("aria-expanded", String(!list.hidden));
      });
      box.replaceChildren(head, list);
    })
    .catch(() => {
      // unparsable: the raw text stays
    });
  return box;
}

// ----------------------------------------------------------------- saving

/** Heart or flag, like saving a place in Google Maps: always one of the two. */
function saveButtons(s: NonNullable<PlaceCardOptions["saved"]>) {
  const box = el("div", "place-save");
  const add = (visited: boolean, path: string, text: string, cls: string) => {
    const b = el("button", `place-save-btn ${cls}`);
    b.type = "button";
    b.setAttribute("aria-pressed", String(s.visited === visited));
    b.append(icon(path), text);
    b.addEventListener("click", (e) => {
      e.preventDefault();
      e.stopPropagation();
      if (s.visited !== visited) s.set(visited);
    });
    box.append(b);
  };
  add(true, HEART, "Been here", "is-been");
  add(false, FLAG, "Want to go", "is-want");
  return box;
}

// --------------------------------------------------------------------- card

export function placeCard(o: PlaceCardOptions): HTMLElement {
  const { url, place, details: d } = o;
  const card = el("div", "place-card");
  if (o.compact) card.classList.add("is-compact");
  // Stops a click on text or padding from reaching the editor or the map.
  card.addEventListener("click", (e) => e.stopPropagation());

  const name = d?.name ?? place.name ?? "A place";
  const body = el("div", "place-body");

  if (d?.photo) {
    const img = el("img", "place-photo");
    img.alt = "";
    img.loading = "lazy";
    img.decoding = "async";
    // A broken photo just goes away; the card keeps its text.
    img.addEventListener("error", () => img.remove());
    img.src = d.photo;
    card.append(img);
  }

  const head = el("div", "place-head");
  head.append(el("div", "place-name", name));
  if (d?.kind) head.append(el("div", "place-kind", d.kind));
  body.append(head);
  if (o.saved) body.append(saveButtons(o.saved));

  if (!d) body.append(el("div", "place-coords", coords(place.lat, place.lng)));
  if (d?.opening_hours) body.append(hoursBlock(d.opening_hours, place));
  if (d?.address) body.append(row("Address", d.address));
  const site = d?.website ? webHost(d.website) : null;
  if (d?.website && site) body.append(row("Website", link(d.website, site)));
  if (d?.phone) body.append(row("Phone", link(`tel:${d.phone.replace(/[^\d+]/g, "")}`, d.phone)));

  if (d?.description) {
    const p = el("p", "place-desc", d.description);
    if (d.wikipedia && webHost(d.wikipedia)) p.append(" ", link(d.wikipedia, "Wikipedia"));
    body.append(p);
  }

  const foot = el("div", "place-foot");
  foot.append(link(url, mapsLabel(url)));
  if (d?.osm && d.osm !== url && webHost(d.osm)) foot.append(link(d.osm, "OpenStreetMap"));
  body.append(foot);

  card.append(body);
  return card;
}

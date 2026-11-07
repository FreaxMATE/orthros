// Country borders from the bundled Natural Earth file (public/countries.geojson):
// which country contains a point, and which country a typed line names.
// Loaded once; everything works offline.

import { stripMarkup } from "./places";

export interface Country {
  name: string;
  long: string;
  /** German name. */
  de: string;
  iso: string;
}

type Ring = number[][];
interface Shape {
  country: Country;
  /** Polygons, each an outer ring followed by holes. */
  polys: Ring[][];
  bbox: [number, number, number, number];
}

export interface Collection {
  type: "FeatureCollection";
  features: unknown[];
}

let loading: Promise<Collection> | null = null;
let shapes: Shape[] = [];

export function loadCountries(): Promise<Collection> {
  loading ??= (async () => {
    const res = await fetch("/countries.geojson");
    if (!res.ok) throw new Error(`countries.geojson: ${res.status}`);
    const fc = (await res.json()) as Collection;
    shapes = fc.features.map((f: any) => {
      const g = f.geometry;
      const polys: Ring[][] = g.type === "Polygon" ? [g.coordinates] : g.coordinates;
      const bbox: Shape["bbox"] = [Infinity, Infinity, -Infinity, -Infinity];
      for (const p of polys)
        for (const [x, y] of p[0]) {
          bbox[0] = Math.min(bbox[0], x);
          bbox[1] = Math.min(bbox[1], y);
          bbox[2] = Math.max(bbox[2], x);
          bbox[3] = Math.max(bbox[3], y);
        }
      const p = f.properties ?? {};
      const country: Country = { name: p.name ?? "", long: p.long ?? "", de: p.de ?? "", iso: p.iso ?? "" };
      return { country, polys, bbox };
    });
    return fc;
  })();
  // A failed load may be retried on the next call.
  loading.catch(() => (loading = null));
  return loading;
}

/** Ray casting: odd number of edge crossings means inside. */
function inRing(ring: Ring, x: number, y: number): boolean {
  let inside = false;
  for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) {
    const [xi, yi] = ring[i];
    const [xj, yj] = ring[j];
    if (yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) inside = !inside;
  }
  return inside;
}

/** The country containing the point, if any (needs loadCountries() done). */
export function countryAt(lng: number, lat: number): Country | null {
  for (const s of shapes) {
    const b = s.bbox;
    if (lng < b[0] || lng > b[2] || lat < b[1] || lat > b[3]) continue;
    for (const p of s.polys) if (inRing(p[0], lng, lat) && !p.slice(1).some((h) => inRing(h, lng, lat))) return s.country;
  }
  return null;
}

/** The country a line names (English, long or German name, or ISO code,
 *  any case), ignoring list markers and link syntax. */
export function findCountry(text: string): Country | null {
  const t = stripMarkup(text).toLowerCase();
  if (!t) return null;
  const hit = (c: Country) => [c.name, c.long, c.de, c.iso].some((n) => n && n.toLowerCase() === t);
  return shapes.find((s) => hit(s.country))?.country ?? null;
}

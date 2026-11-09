// The two marks of a saved place, as in Google Maps: a heart where you have
// been, a flag where you want to go. Paths on a 24x24 grid, drawn as SVG on
// the card and into a canvas for the map's pins (no font has both).

export const HEART =
  "M12 21.35l-1.45-1.32C5.4 15.36 2 12.28 2 8.5 2 5.42 4.42 3 7.5 3c1.74 0 3.41.81 4.5 2.09C13.09 3.81 14.76 3 16.5 3 19.58 3 22 5.42 22 8.5c0 3.78-3.4 6.86-8.55 11.54L12 21.35z";
export const FLAG = "M14.4 6L14 4H5v17h2v-7h5.6l.4 2h7V6z";

const SVG = "http://www.w3.org/2000/svg";

export function icon(path: string) {
  const svg = document.createElementNS(SVG, "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("aria-hidden", "true");
  const p = document.createElementNS(SVG, "path");
  p.setAttribute("d", path);
  svg.append(p);
  return svg;
}

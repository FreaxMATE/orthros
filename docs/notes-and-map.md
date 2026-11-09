# Notes and map

Orthros grows from a daily todo journal into a small personal space: days,
notes and a map of places. It stays simple. The todo core (Today, days, quick
capture, sync) keeps working exactly as now, and everything new is plain
markdown in the same folder, synced by the same server.

## Notes

- **Tabs:** Today | Notes | Map | Days | Search.
- **Later becomes a pinned note.** It keeps its file `later.md`, so sync and
  existing spaces are unaffected. It is always first in the notes list, and
  dragging a task onto the Notes tab drops it into Later.
- **Notes tab:** a flat list. Later first, then pinned notes, then the rest by
  last edit. "+ New note" at the top. Clicking a note opens it in place, with
  a way back to the list.
- **Files:** `notes/<slug>.md`. The name typed when creating a note becomes the
  first line (`# Trip ideas`) and, slugified, the file name (`trip-ideas`).
  The list shows the first heading as the title, so editing the heading
  renames the note in the list; the file name stays.
- **Pins:** `pins.md`, one note name per line (`notes/life-goals`). Plain text,
  so pins sync and merge like everything else.
- **Deleting:** emptying a note removes its file, as with days today.
- **Editing:** same editor as Later. Enter continues a plain bullet, `[] `
  makes a checkbox. Tables, code and math work as everywhere else.
- **Life goals:** just a pinned note, nothing special.
- **Search** covers notes.

## Link cards

- A line whose only content is a URL (`https://...` or `- https://...`) shows
  as a card: image, title, description, site. Links written as `[text](url)`
  stay inline links. The URL shows as text while the cursor is on the line.
- The app (Rust side) fetches the page once, reads its `og:` / `<title>`
  metadata, downloads the image and stores everything under
  `<root>/.cache/links/` (local, never synced). Images reach the webview as
  `data:` URLs, so the content security policy stays strict and cards work
  offline once fetched.
- Offline and not cached yet: a plain link, retried later.

## Map

- **Pins** come from links in any note that point to a place: Google Maps
  links (short `maps.app.goo.gl` links are resolved once while online, then
  cached) and plain coordinates.
- **Want to go or been there is the checkbox:** `- [ ] [Kyoto](link)` is a
  green flag, `- [x] [Kyoto](link)` a red heart, as in Google Maps. A place
  link without a checkbox counts as want to go. The place card has a heart
  and a flag button that set the checkbox. Clicking a pin opens its note at that
  line.
- **Visited countries are shaded** while zoomed out far enough to see at
  least a continent (they fade out between zoom 3 and 4), from ticked places (country found offline
  from bundled Natural Earth borders) and from clicking a country on the map,
  which writes `- [x] Portugal` into a normal "Been there" note.
- **Layers** (MapLibre): two modes, Streets and Satellite. Satellite is a
  hybrid like Google's: imagery with roads, borders and names from the
  street map on top. Bundled country outlines lie under both and are all
  that shows while offline before the street map was ever cached; they are
  not a mode of their own. Viewed street areas are cached; satellite is
  online only.

## Android

- The full Orthros (Today, Notes, Map) via Tauri 2 on Android, offline first
  with the same sync.
- Share sheet: share text or a link into Today or Later, or into a chosen
  note. Google Maps shares land here.

## Order of work

1. Notes tab, Later as a note, link cards, search over notes (desktop).
2. Map: pins, planned/visited, country shading, layers.
3. Android: phone layout, share sheet, sync on the phone.

## Compatibility

Document names are validated in `core` (`doc_path`) by the app and the
server alike. A version that does not know `notes/...` or `pins` rejects them:
an old server refuses the upload, an old app fails to store the download and
its sync stays in error. So a release with notes needs the server and every
device updated together. The release notes must say so.

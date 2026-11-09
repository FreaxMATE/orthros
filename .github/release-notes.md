Orthros is now more than a todo list: it has notes, a map of your places, and an Android app.

**Before you update:** if you use sync, update the server and all your devices together. Older versions don't know notes and stop syncing when they meet one.

## New

- **Notes.** A Notes tab for trip ideas, life goals, books to read. Pin the ones you want on top. Later is now the first note, and dragging a task onto the Notes tab still drops it there.
- **Map.** Every Google Maps, OpenStreetMap or Apple Maps link in a note becomes a pin: a green flag where you want to go, a red heart where you've been. Switch between Streets and Satellite. Countries you've been to are coloured in when zoomed out; click a country to add it. Country borders work offline, and street tiles you've looked at are kept.
- **Place cards.** A line that is only a map link shows the place: photo, opening hours with "open now", address, website, phone and a short Wikipedia summary, from OpenStreetMap and Wikipedia. Set **Been here** or **Want to go** right on the card.
- **Link cards.** A line that is only a link shows the page's title, image and description.
- **Android.** Today, Notes, Map and Search on your phone, with a bar for indenting and moving items while you type. To sync, click the dot at the top right on your computer, choose **Connect a phone**, and scan the code with the phone.

## Changed

- Search also finds text in notes.
- A click on a link always opens it, also on the line you're editing.
- The sync dot opens a small panel with the status and **Sync now**.
- Bullets, checkboxes and the drag handle line up with the text.

## Updating

- **Sync server:** update it too, together with your devices.
- **macOS:** quit Orthros from its menu bar icon, download the new `.dmg` and replace the app in Applications. Then run the `xattr` command below once more.
- **Linux:** quit Orthros from its top bar icon (closing the window only hides it), then install the new package below.
- **Nix:** run `nix flake update orthros`, rebuild, then quit and reopen Orthros.
- **Android:** install the APK. Your notes stay on the phone until you connect it to your sync server.

Map data: [Natural Earth](https://www.naturalearthdata.com), [OpenFreeMap](https://openfreemap.org) with data from [OpenStreetMap](https://www.openstreetmap.org/copyright), satellite imagery from Esri.

## Install the app

| | Download |
|---|---|
| **macOS**, Apple silicon | `Orthros_*_aarch64.dmg` |
| **macOS**, Intel | `Orthros_*_x64.dmg` |
| **Linux**, any distro | `Orthros_*_amd64.AppImage` or `Orthros_*_x86_64.flatpak` |
| **Debian, Ubuntu** | `Orthros_*_amd64.deb` |
| **Fedora, openSUSE** | `Orthros-*.x86_64.rpm` |
| **Nix** | `nix run github:FreaxMATE/orthros` |
| **Android**, arm64 | `Orthros_*_arm64.apk` |

macOS: the app isn't signed. After moving it to Applications, run once:
`xattr -dr com.apple.quarantine /Applications/Orthros.app`

## Sync server (optional)

```sh
docker run -d --name orthros -p 127.0.0.1:8787:8787 -v orthros:/data ghcr.io/freaxmate/orthros-server
```

or `curl -fsSL https://freaxmate.github.io/orthros/install-server.sh | sh` on a Linux server, or the `orthros-server-*` binaries below. See the [README](https://github.com/FreaxMATE/orthros#sync-your-devices).

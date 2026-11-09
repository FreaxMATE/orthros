// The sync popover: a small panel under the sync dot with the status, "Sync
// now", and the pairing between desktop and phone. The desktop shows a QR code
// with the server and token; the phone scans it and stores them.

import qrcode from "qrcode-generator";
import { api, isMobile } from "./api";

interface Options {
  button: HTMLButtonElement;
  syncNow: () => void;
}

/** What the QR code carries; `orthros: 1` is the format version. */
function parsePairing(text: string): { server: string; token: string } {
  let data: { orthros?: unknown; server?: unknown; token?: unknown };
  try {
    data = JSON.parse(text);
  } catch {
    throw new Error("That is not an Orthros code.");
  }
  if (!data || data.orthros !== 1) throw new Error("That is not an Orthros code.");
  const { server, token } = data;
  if (typeof server !== "string" || typeof token !== "string" || !token.trim()) throw new Error("The code is incomplete.");
  let url: URL;
  try {
    url = new URL(server);
  } catch {
    throw new Error("The code has no valid server address.");
  }
  if (url.protocol !== "https:") throw new Error("The server address must be https.");
  return { server, token };
}

function qrSvg(server: string, token: string) {
  const qr = qrcode(0, "M");
  qr.addData(JSON.stringify({ orthros: 1, server, token }));
  qr.make();
  return qr.createSvgTag({ cellSize: 4, margin: 4, scalable: true });
}

export function mountSyncPopover({ button, syncNow }: Options) {
  const panel = document.createElement("div");
  panel.className = "sync-panel";
  panel.id = "sync-panel";
  panel.setAttribute("role", "dialog");
  panel.setAttribute("aria-label", "Sync");
  panel.hidden = true;
  button.setAttribute("aria-controls", panel.id);
  button.after(panel);

  const status = document.createElement("p");
  status.className = "sync-status";
  const now = action("Sync now", () => syncNow());
  const extra = document.createElement("div");
  extra.className = "sync-extra";
  panel.append(status, now, extra);
  // The phone has no top bar to keep the theme switch in.
  if (isMobile) panel.append(document.querySelector("#theme-switch")!);

  function action(label: string, onclick: () => void) {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "sync-action";
    b.textContent = label;
    b.onclick = onclick;
    return b;
  }

  const line = (text: string, cls = "sync-note") => {
    const p = document.createElement("p");
    p.className = cls;
    p.textContent = text;
    return p;
  };

  async function fill() {
    const info = await api.syncInfo();
    if (panel.hidden) return;
    if (isMobile) return fillMobile(info?.server ?? null);
    if (!info) return extra.replaceChildren(line("Set up sync first: see the README."));
    const show = action("Connect a phone", () => {
      const box = document.createElement("div");
      box.className = "sync-qr";
      // Dark modules on a white square in every theme: scanners need it.
      box.innerHTML = qrSvg(info.server, info.token);
      box.querySelector("svg")?.setAttribute("aria-label", "QR code for connecting a phone");
      extra.replaceChildren(box, line("Scan with Orthros on your phone"));
    });
    extra.replaceChildren(show);
  }

  function fillMobile(server: string | null) {
    const err = line("", "sync-note sync-error");
    err.setAttribute("role", "alert");
    const scanBtn = action(server ? "Scan again" : "Scan to connect", async () => {
      err.textContent = "";
      scanBtn.disabled = true;
      try {
        const { checkPermissions, requestPermissions, scan, Format } = await import("@tauri-apps/plugin-barcode-scanner");
        let perm = await checkPermissions();
        if (perm !== "granted") perm = await requestPermissions();
        if (perm !== "granted") throw new Error("Camera access is needed to scan the code.");
        const { content } = await scan({ windowed: false, formats: [Format.QRCode] });
        const c = parsePairing(content);
        await api.connectSync(c.server, c.token);
        await fill();
      } catch (e) {
        err.textContent = e instanceof Error ? e.message : String(e);
      } finally {
        scanBtn.disabled = false;
      }
    });
    const head = server ? [line(`Connected to ${new URL(server).host}`)] : [];
    extra.replaceChildren(...head, scanBtn, err);
  }

  function open() {
    panel.hidden = false;
    button.setAttribute("aria-expanded", "true");
    extra.replaceChildren();
    void fill();
  }

  function close(refocus = false) {
    if (panel.hidden) return;
    panel.hidden = true;
    button.setAttribute("aria-expanded", "false");
    if (refocus) button.focus();
  }

  button.addEventListener("click", () => (panel.hidden ? open() : close()));
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !panel.hidden) close(true);
  });
  document.addEventListener("pointerdown", (e) => {
    if (!panel.hidden && !panel.contains(e.target as Node) && !button.contains(e.target as Node)) close();
  });

  return {
    /** The current status sentence (see renderSync). */
    setStatus(text: string) {
      status.textContent = text;
    },
  };
}

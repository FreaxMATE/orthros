// Quick capture: type a thought, Enter saves it to Later (a bullet) or to
// today (a task), Tab switches between the two, Esc closes.

import "@fontsource/source-serif-4/400.css";
import "@fontsource/source-serif-4/400-italic.css";
import "@fontsource/ibm-plex-mono/400.css";
import "./tokens.css";
import "./capture.css";
import "./theme";

import { api, inTauri } from "./api";
import { today } from "./dates";

type Target = "later" | "today";

const form = document.querySelector<HTMLFormElement>("#capture")!;
const input = document.querySelector<HTMLInputElement>("#thought")!;
const radios = [...document.querySelectorAll<HTMLButtonElement>("[data-target]")];
let target: Target = "later";

function setTarget(t: Target) {
  target = t;
  for (const r of radios) r.setAttribute("aria-checked", String(r.dataset.target === t));
  input.placeholder = t === "later" ? "A thought for later…" : "A task for today…";
  input.focus();
}

function reset() {
  input.value = "";
  setTarget("later");
}

async function close() {
  reset();
  await api.closeCapture();
}

radios.forEach((r) => (r.onclick = () => setTarget(r.dataset.target as Target)));

form.addEventListener("submit", async (e) => {
  e.preventDefault();
  const text = input.value.trim();
  if (!text) return close();
  await api.capture(text, target, today());
  reset();
});

window.addEventListener("keydown", (e) => {
  if (e.key === "Escape") {
    e.preventDefault();
    void close();
  } else if (e.key === "Tab") {
    e.preventDefault();
    setTarget(target === "later" ? "today" : "later");
  }
});

if (inTauri) {
  void import("@tauri-apps/api/window").then(({ getCurrentWindow }) =>
    getCurrentWindow().onFocusChanged(({ payload: focused }) => {
      if (focused) input.focus();
      // Clicking elsewhere closes an empty popup; typed text is never thrown away.
      else if (!input.value.trim()) void close();
    }),
  );
}

reset();

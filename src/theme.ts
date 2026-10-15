// Light / dark switch: "system" follows the desktop, or force one. Remembered
// per device in localStorage (a viewing preference, not part of your data).
//
// "system" is resolved here and always written as an explicit data-theme.
// The desktop's preference comes from the Rust side when it knows it (Linux:
// the portal's color-scheme, which WebKitGTK itself misreads under a light
// GTK3 theme), else from the webview's prefers-color-scheme (macOS, browser).

export type Theme = "system" | "light" | "dark";
type Scheme = "light" | "dark";

const KEY = "orthros:theme";
const ORDER: Theme[] = ["system", "light", "dark"];
const inTauri = "__TAURI_INTERNALS__" in window;
const media = window.matchMedia("(prefers-color-scheme: dark)");

let current = load();
let desktop: Scheme | null = null;

function load(): Theme {
  try {
    const t = localStorage.getItem(KEY);
    if (t === "light" || t === "dark") return t;
  } catch {
    // storage unavailable: follow the system
  }
  return "system";
}

function resolve(): Scheme {
  if (current !== "system") return current;
  return desktop ?? (media.matches ? "dark" : "light");
}

function apply() {
  const scheme = resolve();
  document.documentElement.dataset.theme = scheme;
  // Keep the native title bar (GTK header bar / macOS) in step.
  if (inTauri) {
    void import("@tauri-apps/api/window").then(({ getCurrentWindow }) =>
      getCurrentWindow().setTheme(scheme).catch(() => {}),
    );
  }
}

apply();
media.addEventListener("change", apply);

if (inTauri) {
  void (async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    const { listen } = await import("@tauri-apps/api/event");
    await listen<Scheme | null>("system-theme", (e) => {
      desktop = e.payload;
      apply();
    });
    desktop = await invoke<Scheme | null>("system_theme");
    apply();
  })();
}

export function wireThemeSwitch(button: HTMLButtonElement) {
  const render = () => {
    button.textContent = current;
    button.setAttribute("aria-label", `Theme: ${current}. Click to change.`);
    button.title = "Switch theme: system, light, dark";
  };
  render();
  button.onclick = () => {
    current = ORDER[(ORDER.indexOf(current) + 1) % ORDER.length];
    try {
      localStorage.setItem(KEY, current);
    } catch {
      // not remembered, but still applied
    }
    apply();
    render();
  };
}

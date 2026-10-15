// The bridge to the Rust core. Outside Tauri (plain `npm run dev` in a
// browser) it falls back to localStorage so the UI can be worked on alone.

export interface Hit {
  doc: string;
  line: number;
  text: string;
}

const inTauri = "__TAURI_INTERNALS__" in window;

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

const KEY = "orthros:";

// `?demo` in the browser: start from a sample space (UI work, screenshots).
if (!inTauri && new URLSearchParams(location.search).has("demo")) {
  const d = (n: number) => {
    const t = new Date();
    t.setDate(t.getDate() + n);
    return t.toLocaleDateString("sv-SE"); // YYYY-MM-DD
  };
  const sample: Record<string, string> = {
    [`days/${d(1)}`]: "- [ ] Send the draft to my supervisor\n",
    [`days/${d(0)}`]: [
      "- [x] Understand the structure of the statistics section",
      "- [ ] Write the statistics section",
      "  - [ ] Describe the **hypothesis tests**",
      "- [ ] Investigate _January_ outliers",
      "- [ ] Add regressors to the model",
      "  - [x] Cross-border capacity",
      "  - [ ] Price lags, see `lag_features.py`",
      "## Notes",
      "- Ask about outlier treatment in the [ENTSO-E docs](https://transparency.entsoe.eu)",
      "",
    ].join("\n"),
    [`days/${d(-1)}`]: "- [x] Load ENTSO-E data for 2023\n- [ ] Read the price forecasting review paper\n",
    later: "- Quantile regression for price spikes?\n- Paper on the merit-order effect\n- Try a LightGBM baseline\n",
  };
  for (const [k, v] of Object.entries(sample)) localStorage.setItem(KEY + k, v);
}

const browser = {
  read: async (name: string) => localStorage.getItem(KEY + name) ?? "",
  write: async (name: string, content: string) => {
    if (content.trim()) localStorage.setItem(KEY + name, content);
    else localStorage.removeItem(KEY + name);
  },
  days: async () =>
    Object.keys(localStorage)
      .filter((k) => k.startsWith(KEY + "days/"))
      .map((k) => k.slice(KEY.length + 5))
      .sort()
      .reverse(),
  search: async (query: string) => {
    const q = query.trim().toLowerCase();
    if (!q) return [];
    const docs = ["later", ...(await browser.days()).map((d) => `days/${d}`)];
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
  write: (name: string, content: string) =>
    inTauri ? call<void>("write_doc", { name, content }) : browser.write(name, content),
  days: () => (inTauri ? call<string[]>("list_days") : browser.days()),
  search: (query: string) => (inTauri ? call<Hit[]>("search", { query }) : browser.search(query)),
};

export async function openUrl(url: string) {
  if (inTauri) {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(url);
  } else {
    window.open(url, "_blank", "noopener");
  }
}

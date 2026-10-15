// Local calendar dates as YYYY-MM-DD strings, the format of day file names.

export function iso(d: Date): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

export function parse(s: string): Date {
  const [y, m, d] = s.split("-").map(Number);
  return new Date(y, m - 1, d);
}

export function addDays(s: string, n: number): string {
  const d = parse(s);
  d.setDate(d.getDate() + n);
  return iso(d);
}

export const today = () => iso(new Date());

/** "Today", "Tomorrow", "Yesterday", else "Monday, 12 October". */
export function title(s: string): string {
  const t = today();
  if (s === t) return "Today";
  if (s === addDays(t, 1)) return "Tomorrow";
  if (s === addDays(t, -1)) return "Yesterday";
  return parse(s).toLocaleDateString("en-GB", { weekday: "long", day: "numeric", month: "long" });
}

/** "2026-10-15 · Thursday" */
export function meta(s: string): string {
  return `${s} · ${parse(s).toLocaleDateString("en-GB", { weekday: "long" })}`;
}

// The editing bar of the phone: outdent, indent, task, move up, move down.
// It stands in for Shift-Tab, Tab, Ctrl-Enter and Alt-Up/Down, and runs the
// same commands on whichever editor has focus. It sits just above the
// on-screen keyboard (the visual viewport's bottom) while an editor is focused.

import { indent, outdent, toggle } from "./editor/keys";
import type { DocEditor } from "./editor/editor";

export function mountEditBar(focused: () => DocEditor | undefined) {
  const bar = document.createElement("div");
  bar.className = "edit-bar";
  bar.hidden = true;
  bar.setAttribute("role", "toolbar");
  bar.setAttribute("aria-label", "Editing");

  const buttons: [string, string, (e: DocEditor) => void][] = [
    ["⇤", "Outdent", (e) => outdent(e.view)],
    ["⇥", "Indent", (e) => indent(e.view)],
    ["☐", "Toggle task", (e) => toggle(e.view)],
    ["↑", "Move up", (e) => e.move(-1)],
    ["↓", "Move down", (e) => e.move(1)],
  ];
  for (const [glyph, label, run] of buttons) {
    const b = document.createElement("button");
    b.type = "button";
    b.textContent = glyph;
    b.setAttribute("aria-label", label);
    b.title = label;
    // Pressing must not blur the editor (that would close the keyboard).
    b.addEventListener("pointerdown", (e) => e.preventDefault());
    b.addEventListener("mousedown", (e) => e.preventDefault());
    b.addEventListener("click", () => {
      const ed = focused();
      if (ed) run(ed);
    });
    bar.append(b);
  }
  document.body.append(bar);

  // Keep the bar at the bottom of what is visible: the keyboard shrinks the
  // visual viewport, and the bar rides on top of it.
  const place = () => {
    const vv = window.visualViewport;
    const covered = vv ? Math.max(0, window.innerHeight - vv.height - vv.offsetTop) : 0;
    // With the keyboard up the tab bar is hidden and the bar sits on the keyboard;
    // otherwise it sits on the tab bar (see style.css).
    const open = covered > 80;
    document.documentElement.style.setProperty("--kb", `${covered}px`);
    document.documentElement.classList.toggle("keyboard-open", open);
  };
  window.visualViewport?.addEventListener("resize", place);
  window.visualViewport?.addEventListener("scroll", place);

  // focusout then focusin when focus hops between editors: settle first.
  const update = () =>
    window.setTimeout(() => {
      const on = !!document.activeElement?.closest(".cm-editor");
      bar.hidden = !on;
      document.documentElement.classList.toggle("is-editing", on);
      place();
    }, 0);
  document.addEventListener("focusin", update);
  document.addEventListener("focusout", update);
}

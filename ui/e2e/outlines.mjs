// Gray-outline sweep for the shots harness (LO-UI-5).
//
// Lists every rendered element whose border is a neutral gray or white
// (not red-family, not transparent). The Loadout palette draws container
// outlines in red / red-dim; amber marks the one CTA, objectives and warnings.
// A neutral outline is a leftover from the old gray hairlines.

/** Runs in the page: `[{ el, color, text }]` for each gray-outlined element. */
export function sweep() {
  const out = [];
  const nums = (c) => (c.match(/[\d.]+/g) || []).map(Number);
  for (const el of document.querySelectorAll("body *")) {
    if (el.closest(".cm-editor, .xterm, svg")) continue;
    const box = el.getBoundingClientRect();
    if (box.width < 2 || box.height < 2) continue;
    const cs = getComputedStyle(el);
    if (cs.visibility === "hidden" || cs.display === "none" || Number(cs.opacity) === 0) continue;
    for (const side of ["Top", "Right", "Bottom", "Left"]) {
      if (parseFloat(cs[`border${side}Width`]) <= 0) continue;
      const style = cs[`border${side}Style`];
      if (style === "none" || style === "hidden") continue;
      const [r, g, b, a = 1] = nums(cs[`border${side}Color`]);
      if (a < 0.05) continue;
      const spread = Math.max(r, g, b) - Math.min(r, g, b);
      // Neutral: no hue to speak of, and light enough to read as a line.
      if (spread < 24 && Math.max(r, g, b) > 40) {
        const cls = typeof el.className === "string" && el.className.trim() ? `.${el.className.trim().split(/\s+/).join(".")}` : "";
        const label = el.getAttribute("aria-label");
        out.push({
          el: `${el.tagName.toLowerCase()}${cls}${label ? `[${label}]` : ""}`,
          color: cs[`border${side}Color`],
          text: (el.textContent || "").trim().slice(0, 30),
        });
        break;
      }
    }
  }
  return out;
}

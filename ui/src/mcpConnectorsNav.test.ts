import { describe, expect, it } from "vitest";
import { VIEW_LABELS, paletteRows, railViews } from "./Palette";
import { SECTION_LABELS, sections, type Section } from "./sections";
import type { Capabilities } from "./api";

const DESKTOP: Capabilities = {
  desktopControl: true,
  pulse: true,
  localAi: true,
  tray: true,
  webAccess: true,
  plugins: true,
  library: true,
  autoSetup: true,
  code: true,
  mods: true,
};

describe("MCPs & Connectors nav", () => {
  it("keeps the page on the grouped rail", () => {
    expect(railViews(DESKTOP)).toEqual([
      "home",
      "objectives",
      "chat",
      "code",
      "command",
      "history",
      "pulse",
      "journal",
      "meetings",
      "automations",
      "agents",
      "team",
      "memory",
      "skills",
      "connectors",
      "marketplace",
      "workboard",
      "devices",
      "fleet",
      "library",
      "mods",
      "updates",
      "settings",
    ]);
    expect(VIEW_LABELS.connectors).toBe("MCPs & Connectors");
  });

  it("leaves connectors and MCP out of Settings sections", () => {
    const ids = sections(DESKTOP);
    expect(ids).not.toContain("connectors" as Section);
    expect(ids).not.toContain("mcp" as Section);
    const labels = ids.map((id) => SECTION_LABELS[id]);
    expect(labels).not.toContain("Connectors");
    expect(labels).not.toContain("MCPs & Connectors");
    expect(labels.some((label) => /mcp/i.test(label))).toBe(false);
  });

  it("old Settings deep links redirect to the page, not a Settings section", () => {
    const rows = paletteRows(DESKTOP, "connectors");
    const settingsHit = rows.find((row) => row.action.kind === "settings");
    expect(settingsHit).toBeUndefined();
    const page = rows.find((row) => row.action.kind === "view" && row.action.view === "connectors");
    expect(page?.label).toBe("MCPs & Connectors");
    expect(page?.action).toEqual({ kind: "view", view: "connectors" });
  });
});

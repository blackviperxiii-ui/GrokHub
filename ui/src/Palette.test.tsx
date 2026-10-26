import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { Capabilities } from "./api";
import { Palette, paletteRows } from "./Palette";

vi.mock("./api", () => ({}));

const DESKTOP: Capabilities = {
  desktopControl: true,
  pulse: true,
  localAi: true,
  tray: true,
  webAccess: true,
  plugins: true,
};
const PHONE: Capabilities = { ...DESKTOP, desktopControl: false, pulse: false, localAi: false, tray: false, webAccess: false, plugins: false };
const WEB: Capabilities = { ...PHONE, remote: true };

const labels = (caps: Capabilities, q: string) => paletteRows(caps, q).map((r) => r.label);

describe("paletteRows", () => {
  it("lists this platform's pages, then its Settings sections", () => {
    expect(labels(DESKTOP, "")).toEqual([
      "Chat",
      "Pulse",
      "Journal",
      "Automations",
      "Memory",
      "Connectors",
      "Settings",
      "Settings: General",
      "Settings: Account",
      "Settings: Appearance",
      "Settings: Personality",
      "Settings: Playbooks",
      "Settings: Permissions",
      "Settings: Pulse",
      "Settings: Local AI",
      "Settings: Web access",
      "Settings: Labs",
      "Settings: About",
    ]);
    expect(labels(PHONE, "")).toEqual([
      "Chat",
      "Journal",
      "Automations",
      "Memory",
      "Connectors",
      "Settings",
      "Settings: General",
      "Settings: Account",
      "Settings: Appearance",
      "Settings: Personality",
      "Settings: Playbooks",
      "Settings: Permissions",
      "Settings: About",
    ]);
    expect(labels(WEB, "")).toEqual(["Chat", "Automations", "Memory", "Connectors", "Settings"]);
  });

  it("filters by the typed text and ends with Ask Grok", () => {
    expect(labels(DESKTOP, "  ACCO ")).toEqual(["Settings: Account", "Ask Grok: ACCO"]);
    expect(paletteRows(DESKTOP, "zzz")).toEqual([
      { label: "Ask Grok: zzz", action: { kind: "ask", text: "zzz" } },
    ]);
  });
});

describe("Palette", () => {
  it("moves with the arrows, wraps, and picks with Enter", async () => {
    const onPick = vi.fn();
    const onClose = vi.fn();
    render(<Palette caps={PHONE} onPick={onPick} onClose={onClose} />);
    expect(screen.getByLabelText("Search pages and commands")).toHaveFocus();
    expect(screen.getByRole("option", { name: "Chat" })).toHaveAttribute("aria-selected", "true");
    await userEvent.keyboard("{ArrowUp}");
    expect(screen.getByRole("option", { name: "Settings: About" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    await userEvent.keyboard("{ArrowDown}{ArrowDown}{Enter}");
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(onPick).toHaveBeenCalledWith({ kind: "view", view: "journal" });
  });

  it("Escape and a click outside close it without a pick", async () => {
    const onPick = vi.fn();
    const onClose = vi.fn();
    const { container } = render(<Palette caps={PHONE} onPick={onPick} onClose={onClose} />);
    await userEvent.keyboard("{Escape}");
    await userEvent.click(container.querySelector(".palette-backdrop")!);
    await userEvent.click(screen.getByRole("dialog"));
    expect(onClose).toHaveBeenCalledTimes(2);
    expect(onPick).not.toHaveBeenCalled();
  });
});

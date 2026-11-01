import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
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
      "Agents",
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
      "Agents",
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
    expect(labels(WEB, "")).toEqual(["Chat", "Automations", "Agents", "Memory", "Connectors", "Settings"]);
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

  describe("with layout", () => {
    afterEach(() => {
      vi.restoreAllMocks();
      vi.unstubAllGlobals();
    });

    /** Rows 36 px tall, 40 px apart, in a list that shows 100 px. */
    function layout(reduced: boolean) {
      vi.stubGlobal(
        "matchMedia",
        vi.fn(() => ({ matches: reduced, addEventListener: () => {}, removeEventListener: () => {} })),
      );
      const row = (el: HTMLElement) => Number(/^palette-row-(\d+)$/.exec(el.id)?.[1] ?? 0);
      vi.spyOn(HTMLElement.prototype, "offsetTop", "get").mockImplementation(function (this: HTMLElement) {
        return row(this) * 40;
      });
      vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(36);
      vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(100);
      render(<Palette caps={PHONE} onPick={vi.fn()} onClose={vi.fn()} />);
      const list = screen.getByRole("listbox");
      let top = 0;
      Object.defineProperty(list, "scrollTop", { get: () => top, configurable: true });
      const scrollTo = vi.fn((to: ScrollToOptions) => {
        top = to.top ?? top;
      });
      list.scrollTo = scrollTo as unknown as typeof list.scrollTo;
      return { list, scrollTo, top: () => top };
    }

    it("is cut partway into a row, so the next one peeks", () => {
      const { list } = layout(false);
      // Half of jsdom's 768 px window is 384: rows 0-8 show, row 9 shows half.
      expect(list.style.maxHeight).toBe("378px");
    });

    it("arrow down near the bottom scrolls so the next row shows", async () => {
      const { scrollTo, top } = layout(false);
      await userEvent.keyboard("{ArrowDown}");
      // Row 1 picked: row 2 (80-116) must show in the 100 px view.
      expect(scrollTo).toHaveBeenLastCalledWith({ top: 16, behavior: "smooth" });
      await userEvent.keyboard("{ArrowDown}");
      expect(screen.getByRole("option", { name: "Automations" })).toHaveAttribute("aria-selected", "true");
      // Row 3 ends at 156.
      expect(top()).toBe(56);
      await userEvent.keyboard("{ArrowUp}");
      // Back on row 1: row 0 starts at 0.
      expect(top()).toBe(0);
    });

    it("wrapping up to the last row lands at the bottom, instantly with reduced motion", async () => {
      const { scrollTo } = layout(true);
      await userEvent.keyboard("{ArrowUp}");
      // Row 12 starts at 480, row 13 ends at 556.
      expect(scrollTo).toHaveBeenLastCalledWith({ top: 456, behavior: "auto" });
    });

    it("a hover picks without scrolling", async () => {
      const { scrollTo } = layout(false);
      await userEvent.hover(screen.getByRole("option", { name: "Settings: About" }));
      expect(screen.getByRole("option", { name: "Settings: About" })).toHaveAttribute("aria-selected", "true");
      expect(scrollTo).not.toHaveBeenCalled();
    });
  });
});

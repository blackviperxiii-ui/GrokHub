import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { Capabilities } from "./api";
import { Rail } from "./Rail";

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

describe("Rail", () => {
  it("shows the five groups and their pages, with no Simple or Advanced switch", async () => {
    const onOpen = vi.fn();
    render(
      <Rail
        caps={DESKTOP}
        view="chat"
        pinned={[]}
        collapsed={[]}
        onOpen={onOpen}
        onChatMenu={() => {}}
        onPin={() => {}}
        onToggleSection={() => {}}
      />,
    );
    const nav = screen.getByRole("navigation", { name: "Views" });
    expect(nav).not.toHaveAttribute("data-rail-mode");
    expect(screen.queryByRole("button", { name: "Simple" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Advanced" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "More" })).not.toBeInTheDocument();
    for (const group of ["Core", "Work", "Make", "You", "PC"]) {
      expect(screen.getByRole("button", { name: group })).toHaveAttribute("aria-expanded", "true");
    }
    for (const page of [
      "Home",
      "Chat",
      "Code",
      "Library",
      "Workboard",
      "Automations",
      "Agents",
      "Team",
      "Objectives",
      "Command line",
      "Skills",
      "MCPs & Connectors",
      "Marketplace",
      "Mods",
      "Personal",
      "Pulse",
      "Journal",
      "Meetings",
      "History",
      "Devices",
      "Fleet",
      "Updates & Drivers",
      "Settings",
    ]) {
      expect(screen.getByRole("button", { name: page })).toBeInTheDocument();
    }
    expect(screen.getByRole("button", { name: "Chat" })).toHaveAttribute("aria-current", "page");
    await userEvent.click(screen.getByRole("button", { name: "Automations" }));
    await userEvent.click(screen.getByRole("button", { name: "Skills" }));
    await userEvent.click(screen.getByRole("button", { name: "Workboard" }));
    await userEvent.click(screen.getByRole("button", { name: "Mods" }));
    expect(onOpen.mock.calls.map((c) => c[0])).toEqual(["automations", "skills", "workboard", "mods"]);
  });

  it("collapses a section and keeps arrow keys on the remaining buttons", async () => {
    const onToggle = vi.fn();
    render(
      <Rail
        caps={DESKTOP}
        view="chat"
        pinned={[]}
        collapsed={["make"]}
        onOpen={() => {}}
        onChatMenu={() => {}}
        onPin={() => {}}
        onToggleSection={onToggle}
      />,
    );
    expect(screen.queryByRole("button", { name: "Skills" })).not.toBeInTheDocument();
    const work = screen.getByRole("button", { name: "Work" });
    expect(work).toHaveAttribute("aria-expanded", "true");
    await userEvent.click(work);
    expect(onToggle).toHaveBeenCalledWith("work");
    const chat = screen.getByRole("button", { name: "Chat" });
    chat.focus();
    await userEvent.keyboard("{ArrowDown}");
    expect(document.activeElement).not.toBe(chat);
    expect(document.activeElement?.tagName).toBe("BUTTON");
  });
});

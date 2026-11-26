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
  it("Simple mode shows core pages and More for the rest", () => {
    render(
      <Rail
        caps={DESKTOP}
        view="chat"
        mode="simple"
        pinned={[]}
        collapsed={[]}
        moreOpen={false}
        onOpen={() => {}}
        onMore={() => {}}
        onChatMenu={() => {}}
        onMode={() => {}}
        onPin={() => {}}
        onToggleSection={() => {}}
      />,
    );
    expect(screen.getByRole("navigation", { name: "Views" })).toHaveAttribute("data-rail-mode", "simple");
    expect(screen.getByRole("button", { name: "Chat" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("button", { name: "More" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Automations" })).not.toBeInTheDocument();
  });

  it("Advanced mode lists Automations, Skills, Workboard and Mods in one click", async () => {
    const onOpen = vi.fn();
    render(
      <Rail
        caps={DESKTOP}
        view="home"
        mode="advanced"
        pinned={[]}
        collapsed={[]}
        moreOpen={false}
        onOpen={onOpen}
        onMore={() => {}}
        onChatMenu={() => {}}
        onMode={() => {}}
        onPin={() => {}}
        onToggleSection={() => {}}
      />,
    );
    expect(screen.getByRole("navigation", { name: "Views" })).toHaveClass("rail-advanced");
    expect(screen.queryByRole("button", { name: "More" })).not.toBeInTheDocument();
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
        mode="advanced"
        pinned={[]}
        collapsed={["make"]}
        moreOpen={false}
        onOpen={() => {}}
        onMore={() => {}}
        onChatMenu={() => {}}
        onMode={() => {}}
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

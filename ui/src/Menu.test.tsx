import { act, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Menu, MenuItem, SAFE_TRIANGLE_IDLE_MS, SubMenu } from "./Menu";

function stubRect(el: Element, rect: Partial<DOMRect>) {
  vi.spyOn(el, "getBoundingClientRect").mockReturnValue({
    x: rect.left ?? 0,
    y: rect.top ?? 0,
    left: rect.left ?? 0,
    top: rect.top ?? 0,
    right: rect.right ?? 0,
    bottom: rect.bottom ?? 0,
    width: (rect.right ?? 0) - (rect.left ?? 0),
    height: (rect.bottom ?? 0) - (rect.top ?? 0),
    toJSON() {
      return this;
    },
  });
}

function DemoMenu() {
  return (
    <Menu aria-label="Demo">
      <SubMenu id="files" label="Files">
        <MenuItem>Open</MenuItem>
        <MenuItem>Save</MenuItem>
      </SubMenu>
      <SubMenu id="edit" label="Edit">
        <MenuItem>Cut</MenuItem>
        <MenuItem>Paste</MenuItem>
      </SubMenu>
      <MenuItem>Quit</MenuItem>
    </Menu>
  );
}

describe("Menu safe-triangle corridor", () => {
  beforeEach(() => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("keeps the open submenu open on a diagonal pointer path across a sibling", async () => {
    render(<DemoMenu />);
    const files = screen.getByRole("menuitem", { name: "Files" });
    const edit = screen.getByRole("menuitem", { name: "Edit" });

    // Open Files from its mid-right.
    fireEvent.pointerMove(files, { clientX: 80, clientY: 120 });
    expect(files).toHaveAttribute("aria-expanded", "true");
    const panel = screen.getByRole("menu", { name: "Files" });
    // Submenu opens to the right of the parent column.
    stubRect(panel, { left: 200, top: 100, right: 360, bottom: 220 });

    // Diagonal path across the Edit trigger, still inside the corridor toward the panel.
    fireEvent.pointerMove(edit, { clientX: 140, clientY: 150 });
    expect(files).toHaveAttribute("aria-expanded", "true");
    expect(edit).toHaveAttribute("aria-expanded", "false");
    expect(within(panel).getByRole("menuitem", { name: "Open" })).toBeTruthy();
  });

  it("switches immediately when the pointer leaves the corridor onto a sibling", () => {
    render(<DemoMenu />);
    const files = screen.getByRole("menuitem", { name: "Files" });
    const edit = screen.getByRole("menuitem", { name: "Edit" });

    fireEvent.pointerMove(files, { clientX: 80, clientY: 120 });
    const panel = screen.getByRole("menu", { name: "Files" });
    stubRect(panel, { left: 200, top: 100, right: 360, bottom: 220 });

    // Vertical move well above the corridor.
    fireEvent.pointerMove(edit, { clientX: 90, clientY: 40 });
    expect(edit).toHaveAttribute("aria-expanded", "true");
    expect(files).toHaveAttribute("aria-expanded", "false");
    expect(screen.getByRole("menu", { name: "Edit" })).toBeTruthy();
  });

  it("switches after the idle fallback when paused on a sibling inside the corridor", () => {
    render(<DemoMenu />);
    const files = screen.getByRole("menuitem", { name: "Files" });
    const edit = screen.getByRole("menuitem", { name: "Edit" });

    fireEvent.pointerMove(files, { clientX: 80, clientY: 120 });
    const panel = screen.getByRole("menu", { name: "Files" });
    stubRect(panel, { left: 200, top: 100, right: 360, bottom: 220 });

    fireEvent.pointerMove(edit, { clientX: 140, clientY: 150 });
    expect(files).toHaveAttribute("aria-expanded", "true");
    expect(edit).toHaveAttribute("aria-expanded", "false");

    act(() => {
      vi.advanceTimersByTime(SAFE_TRIANGLE_IDLE_MS);
    });
    expect(edit).toHaveAttribute("aria-expanded", "true");
    expect(files).toHaveAttribute("aria-expanded", "false");
  });

  it("opens and closes nested submenus with ArrowRight, ArrowLeft, and Escape", async () => {
    vi.useRealTimers();
    const user = userEvent.setup();
    const onClose = vi.fn();
    render(
      <Menu aria-label="Demo" onClose={onClose}>
        <SubMenu id="files" label="Files">
          <MenuItem>Open</MenuItem>
        </SubMenu>
        <MenuItem>Quit</MenuItem>
      </Menu>,
    );
    const files = screen.getByRole("menuitem", { name: "Files" });
    files.focus();
    await user.keyboard("{ArrowRight}");
    expect(files).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("menuitem", { name: "Open" })).toHaveFocus();

    await user.keyboard("{ArrowLeft}");
    expect(files).toHaveAttribute("aria-expanded", "false");
    expect(files).toHaveFocus();

    await user.keyboard("{ArrowRight}");
    expect(files).toHaveAttribute("aria-expanded", "true");
    await user.keyboard("{Escape}");
    expect(files).toHaveAttribute("aria-expanded", "false");
    expect(files).toHaveFocus();

    await user.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});

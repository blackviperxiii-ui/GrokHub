import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { DockLayout, DockPane } from "./api";
import {
  Dock,
  cleanLayout,
  cycleFocus,
  emptyLayout,
  focusPage,
  focusedPage,
  moveTab,
} from "./Dock";
import type { View } from "./Palette";

const AVAILABLE: View[] = ["chat", "automations", "skills", "workboard", "pulse"];

const pane = (id: string, tabs: string[], active = 0, share = 1000): DockPane => ({ id, tabs, active, share });

describe("cleanLayout", () => {
  it("keeps a page in one pane only: a duplicate in a saved layout collapses", () => {
    const saved: DockLayout = {
      panes: [pane("a", ["chat", "pulse"], 0, 500), pane("b", ["pulse", "skills"], 0, 500)],
      focused: 1,
    };
    const cleaned = cleanLayout(saved, AVAILABLE);
    const all = cleaned.panes.flatMap((p) => p.tabs);
    expect(all.filter((tab) => tab === "pulse")).toHaveLength(1);
    expect(cleaned.panes[0].tabs).toEqual(["chat", "pulse"]);
    // Pages with no tab yet fold into the focused pane, as in the cabin.
    expect(cleaned.panes[1].tabs).toEqual(["skills", "automations", "workboard"]);
  });

  it("drops pages this platform does not have and clamps the indexes", () => {
    const saved: DockLayout = { panes: [pane("a", ["chat", "pulse"], 9, 0)], focused: 4 };
    const cleaned = cleanLayout(saved, ["chat", "automations"]);
    expect(cleaned).toEqual({
      panes: [pane("a", ["chat", "automations"], 0, 1000)],
      focused: 0,
    });
  });

  it("drops a legacy imagine tab cleanly from an old saved layout", () => {
    const legacy: DockLayout = {
      panes: [pane("a", ["chat", "imagine"], 1, 500), pane("b", ["imagine"], 0, 500)],
      focused: 1,
    };
    const cleaned = cleanLayout(legacy, ["chat", "pulse"]);
    expect(cleaned).toEqual({
      panes: [pane("a", ["chat", "pulse"], 0, 500)],
      focused: 0,
    });
  });

  it("keeps pane names, and gives a fresh one to a pane without", () => {
    const saved: DockLayout = {
      panes: [pane("a", ["chat"], 0, 500), pane("", ["pulse"], 0, 500)],
      focused: 0,
    };
    const cleaned = cleanLayout(saved, ["chat", "pulse"]);
    expect(cleaned.panes[0].id).toBe("a");
    expect(cleaned.panes[1].id).toBeTruthy();
    expect(cleaned.panes[1].id).not.toBe("a");
  });

  it("returns the same layout when nothing had to change", () => {
    const saved: DockLayout = {
      panes: [
        pane("a", ["chat", "automations", "skills", "workboard"], 0, 600),
        pane("b", ["pulse"], 0, 400),
      ],
      focused: 1,
    };
    expect(cleanLayout(saved, AVAILABLE)).toBe(saved);
  });
});

describe("focusPage", () => {
  const two: DockLayout = {
    panes: [pane("a", ["chat", "pulse"], 0, 500), pane("b", ["skills"], 0, 500)],
    focused: 0,
  };

  it("focuses the pane holding the page and makes it the active tab", () => {
    const next = focusPage(two, "pulse");
    expect(next.focused).toBe(0);
    expect(next.panes[0].active).toBe(1);
    expect(focusedPage(next)).toBe("pulse");
    expect(focusPage(focusPage(two, "skills"), "skills").focused).toBe(1);
  });

  it("a page with no tab yet joins the focused pane; a non-dockable page changes nothing", () => {
    const next = focusPage(two, "automations");
    expect(next.panes[0].tabs).toEqual(["chat", "pulse", "automations"]);
    expect(focusedPage(next)).toBe("automations");
    expect(focusPage(two, "settings")).toBe(two);
  });
});

describe("moveTab", () => {
  const two: DockLayout = {
    panes: [pane("a", ["chat", "pulse"], 0, 500), pane("b", ["skills", "workboard"], 0, 500)],
    focused: 0,
  };

  it("moving a tab into another pane makes it that pane's active tab", () => {
    const next = moveTab(two, "pulse", 1, "in");
    expect(next.panes[0].tabs).toEqual(["chat"]);
    expect(next.panes[1].tabs).toEqual(["skills", "workboard", "pulse"]);
    expect(next.focused).toBe(1);
    expect(focusedPage(next)).toBe("pulse");
  });

  it("a pane left empty merges away", () => {
    const one: DockLayout = {
      panes: [pane("a", ["chat"], 0, 500), pane("b", ["skills", "workboard"], 0, 500)],
      focused: 1,
    };
    const next = moveTab(one, "chat", 1, "in");
    expect(next.panes).toHaveLength(1);
    expect(next.panes[0].tabs).toEqual(["skills", "workboard", "chat"]);
    expect(next.panes[0].id).toBe("b");
  });

  it("dropping on an edge splits the pane and halves its share", () => {
    const next = moveTab(two, "chat", 1, "after");
    expect(next.panes.map((p) => p.tabs)).toEqual([["pulse"], ["skills", "workboard"], ["chat"]]);
    expect(next.panes[1].share + next.panes[2].share).toBe(500);
    expect(next.focused).toBe(2);
    // The split-off pane gets its own name; the others keep theirs.
    expect(next.panes[0].id).toBe("a");
    expect(next.panes[1].id).toBe("b");
    expect(next.panes[2].id).not.toBe("a");
    expect(next.panes[2].id).not.toBe("b");
  });
});

describe("cycleFocus", () => {
  it("walks the panes and wraps around", () => {
    const two: DockLayout = {
      panes: [pane("a", ["chat"], 0, 500), pane("b", ["skills"], 0, 500)],
      focused: 0,
    };
    expect(cycleFocus(two, 1).focused).toBe(1);
    expect(cycleFocus(cycleFocus(two, 1), 1).focused).toBe(0);
    expect(cycleFocus(two, -1).focused).toBe(1);
    const single = emptyLayout();
    expect(cycleFocus(single, 1)).toBe(single);
  });
});

describe("Dock", () => {
  const two: DockLayout = {
    panes: [pane("a", ["chat", "pulse"], 1, 600), pane("b", ["skills"], 0, 400)],
    focused: 0,
  };

  it("renders both panes with their active pages", () => {
    render(
      <Dock
        layout={two}
        onLayout={() => {}}
        onFocusPage={() => {}}
        renderPage={(page) => <div>{page} body</div>}
      />,
    );
    expect(screen.getByText("pulse body")).toBeInTheDocument();
    expect(screen.getByText("skills body")).toBeInTheDocument();
    const panes = screen.getAllByRole("tablist", { name: "Pane pages" });
    expect(panes).toHaveLength(2);
    expect(within(panes[0]).getByRole("tab", { name: "Pulse" })).toHaveAttribute("aria-selected", "true");
    expect(within(panes[0]).getByRole("tab", { name: "Chat" })).toHaveAttribute("aria-selected", "false");
  });

  it("clicking a tab or a pane body asks the App to open that page", () => {
    const onFocusPage = vi.fn();
    render(
      <Dock
        layout={two}
        onLayout={() => {}}
        onFocusPage={onFocusPage}
        renderPage={(page) => <div>{page} body</div>}
      />,
    );
    fireEvent.click(screen.getByRole("tab", { name: "Chat" }));
    expect(onFocusPage).toHaveBeenCalledWith("chat");
    fireEvent.mouseDown(screen.getByText("skills body"));
    expect(onFocusPage).toHaveBeenCalledWith("skills");
  });

  it("dropping a tab on another pane moves it there", () => {
    const onLayout = vi.fn();
    const onFocusPage = vi.fn();
    const { container } = render(
      <Dock
        layout={two}
        onLayout={onLayout}
        onFocusPage={onFocusPage}
        renderPage={(page) => <div>{page} body</div>}
      />,
    );
    const tab = screen.getByRole("tab", { name: "Chat" });
    const target = container.querySelectorAll(".dock-pane")[1];
    const data = new Map<string, string>();
    const dataTransfer = {
      types: ["application/x-grokhub-dock-page"],
      setData: (type: string, value: string) => data.set(type, value),
      getData: (type: string) => data.get(type) ?? "",
      effectAllowed: "",
    };
    fireEvent.dragStart(tab, { dataTransfer });
    fireEvent.dragOver(target, { dataTransfer, clientX: 0 });
    fireEvent.drop(target, { dataTransfer, clientX: 0 });
    expect(onLayout).toHaveBeenCalledTimes(1);
    const next = onLayout.mock.calls[0][0] as DockLayout;
    // Wherever in the row the drop landed, Chat left its old pane and is a
    // tab in exactly one pane.
    expect(next.panes.flatMap((p) => p.tabs).filter((tab) => tab === "chat")).toHaveLength(1);
    expect(next.panes.find((p) => p.id === "a")?.tabs).not.toContain("chat");
    expect(onFocusPage).toHaveBeenCalledWith("chat");
  });
});

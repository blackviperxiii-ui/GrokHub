import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { PanelSpec } from "./api";
import { PanelCard, panelActionText, panelTitle, percent } from "./PanelCard";

const EVERY: PanelSpec = {
  title: "Build report",
  elements: [
    { type: "heading", level: 2, text: "Linux CI" },
    { type: "text", text: "Two crates failed to compile." },
    { type: "list", ordered: true, items: ["grokhub-agent", "grokhub-core"] },
    { type: "list", ordered: false, items: ["E0425"] },
    { type: "table", headers: ["Item", "Before", "After"], rows: [["latency", "120ms", "95ms"]] },
    { type: "table", headers: [], rows: [] },
    { type: "key_value", pairs: [{ key: "Workflow", value: "ci" }] },
    { type: "input", id: "branch", label: "Branch", value: "beta" },
    { type: "progress", label: "Tests", value: 0.856 },
    { type: "button", label: "Re-run", action: "rerun_ci" },
  ],
};

describe("PanelCard", () => {
  it("draws every element as plain text, the way the cabin did", () => {
    render(<PanelCard id="call-1" spec={EVERY} onAction={() => {}} onDismiss={() => {}} />);
    const card = screen.getByRole("group", { name: "Build report" });
    expect(within(card).getByText("Linux CI")).toHaveClass("panel-heading");
    expect(within(card).getByText("Two crates failed to compile.")).toBeInTheDocument();
    const [ordered, plain] = within(card).getAllByRole("list");
    expect(ordered.tagName).toBe("OL");
    expect(within(ordered).getAllByRole("listitem").map((li) => li.textContent)).toEqual([
      "grokhub-agent",
      "grokhub-core",
    ]);
    expect(plain.tagName).toBe("UL");
    // A table with no headers is skipped, as in the cabin.
    const tables = within(card).getAllByRole("table");
    expect(tables).toHaveLength(1);
    expect(within(tables[0]).getAllByRole("columnheader").map((th) => th.textContent)).toEqual([
      "Item",
      "Before",
      "After",
    ]);
    expect(within(tables[0]).getAllByRole("cell").map((td) => td.textContent)).toEqual(["latency", "120ms", "95ms"]);
    expect(card).toHaveTextContent("Workflow:ci");
    expect(within(card).getByText("Branch: beta")).toBeInTheDocument();
    expect(within(card).getByText("Tests — 86%")).toBeInTheDocument();
    expect(within(card).getByRole("progressbar", { name: "Tests" })).toHaveAttribute("value", "86");
  });

  it("a button hands up its action id and Dismiss asks to hide the card", async () => {
    const onAction = vi.fn();
    const onDismiss = vi.fn();
    render(<PanelCard id="call-1" spec={EVERY} onAction={onAction} onDismiss={onDismiss} />);
    await userEvent.click(screen.getByRole("button", { name: "Re-run" }));
    expect(onAction).toHaveBeenCalledWith("rerun_ci");
    await userEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it("text that looks like markup stays text", () => {
    render(
      <PanelCard
        id="call-1"
        spec={{ elements: [{ type: "text", text: "<img src=x onerror=alert(1)>" }] }}
        onAction={() => {}}
        onDismiss={() => {}}
      />,
    );
    expect(screen.getByText("<img src=x onerror=alert(1)>")).toBeInTheDocument();
    expect(document.querySelector("img")).toBeNull();
  });
});

describe("panel helpers", () => {
  it("titles a panel by its title, else its first heading, else its id", () => {
    expect(panelTitle("call-1", { title: "  CI  ", elements: [] })).toBe("CI");
    expect(panelTitle("call-1", { elements: [{ type: "heading", level: 2, text: " Checks " }] })).toBe("Checks");
    expect(panelTitle("call-1", { title: " ", elements: [{ type: "text", text: "x" }] })).toBe("Panel call-1");
  });

  it("sends only an action id the core allows, for a panel id it could have made", () => {
    expect(panelActionText("call-1", "rerun_ci")).toBe("[panel action] rerun_ci on call-1");
    expect(panelActionText("call_AbC.9:x", "open-logs2")).toBe("[panel action] open-logs2 on call_AbC.9:x");
    expect(panelActionText("call-1", "Rerun!")).toBeNull();
    expect(panelActionText("call-1", "9lives")).toBeNull();
    expect(panelActionText("call-1", "a".repeat(65))).toBeNull();
    expect(panelActionText("call 1", "rerun_ci")).toBeNull();
    expect(panelActionText("", "rerun_ci")).toBeNull();
  });

  it("rounds progress to a whole percent inside 0..100", () => {
    expect(percent(0.856)).toBe(86);
    expect(percent(1.5)).toBe(100);
    expect(percent(-1)).toBe(0);
    expect(percent(Number.NaN)).toBe(0);
  });
});

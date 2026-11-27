import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { MemoryGraph, MemoryGraphEdge, MemoryGraphNode, MemoryRow } from "./api";

const api = vi.hoisted(() => ({
  getMemoryGraph: vi.fn<() => Promise<MemoryGraph>>(),
}));
vi.mock("./api", () => api);

import {
  MAP_EMPTY,
  MAP_NONE_SHOWN,
  MAP_PRIVATE_NOTE,
  MemoryMap,
  PRIVATE_LABEL,
  ROWS_PER_COLUMN,
  disagreements,
  layoutMap,
  versionsOf,
} from "./MemoryMap";

function node(id: string, over: Partial<MemoryGraphNode> = {}): MemoryGraphNode {
  return {
    id,
    kind: "fact",
    label: `Line ${id}`,
    confidence: 0.9,
    updated: "2026-11-20T00:00:00Z",
    sealed: false,
    proposed: false,
    paused: false,
    superseded: false,
    ...over,
  };
}

const edge = (from: string, to: string, kind: MemoryGraphEdge["kind"]): MemoryGraphEdge => ({ from, to, kind });

function row(id: string, text: string): MemoryRow {
  return {
    id,
    text,
    source: "you said it in chat",
    updated: "2026-11-20T00:00:00Z",
    proposed: false,
    paused: false,
    group: "About you",
    corrects: "",
  };
}

const GRAPH: MemoryGraph = {
  nodes: [
    node("gpu-new", { label: "Has a 5090" }),
    node("gpu-old", { label: "Has a 4090", superseded: true }),
    node("tea", { kind: "preference", label: "Likes tea", proposed: true }),
    node("coffee", { kind: "preference", label: "Likes coffee only" }),
    node("secret", { kind: "person", label: "", sealed: true }),
  ],
  edges: [edge("gpu-new", "gpu-old", "supersedes"), edge("tea", "coffee", "contradicts"), edge("coffee", "tea", "contradicts")],
  locked: 2,
  truncated: false,
};

const ROWS = [row("gpu-new", "Has a 5090"), row("tea", "Likes tea"), row("coffee", "Likes coffee only")];

function renderMap(onKeep = vi.fn()) {
  const renderRow = (r: MemoryRow) => (
    <li key={r.id} aria-label={`Card ${r.text}`}>
      {r.text}
    </li>
  );
  render(<MemoryMap rows={ROWS} renderRow={renderRow} onKeep={onKeep} />);
  return onKeep;
}

const dots = () =>
  within(screen.getByRole("group", { name: /^Memory map/ }))
    .getAllByRole("button")
    .map((b) => b.getAttribute("aria-label"));

// A braced body: a hook that returns a function has it run as a teardown, and
// mockReset returns the mock, which would load the map once more.
beforeEach(() => {
  api.getMemoryGraph.mockReset();
});

describe("layoutMap", () => {
  it("clusters by type in a fixed order and keeps a line of versions together", () => {
    const layout = layoutMap(
      [node("b-old"), node("p", { kind: "preference" }), node("a"), node("b-new")],
      [edge("b-new", "b-old", "supersedes")],
    );
    expect(layout.columns.map((c) => c.heading)).toEqual(["Preferences", "Facts"]);
    const facts = layout.placed.filter((p) => p.node.kind === "fact").map((p) => p.node.id);
    expect(facts).toEqual(["a", "b-new", "b-old"]);
    const xs = new Set(layout.placed.filter((p) => p.node.kind === "fact").map((p) => p.x));
    expect(xs.size).toBe(1);
  });

  it("wraps a long type into a second column with no heading", () => {
    const many = Array.from({ length: ROWS_PER_COLUMN + 3 }, (_, i) => node(`n${i}`));
    const layout = layoutMap(many, []);
    expect(layout.columns.map((c) => c.heading)).toEqual(["Facts", ""]);
    expect(layout.placed).toHaveLength(ROWS_PER_COLUMN + 3);
    expect(layout.width).toBeGreaterThan(layout.columns[1].x);
  });

  it("places every node once even when versions loop", () => {
    const layout = layoutMap([node("x"), node("y")], [edge("x", "y", "supersedes"), edge("y", "x", "supersedes")]);
    expect(layout.placed.map((p) => p.node.id).sort()).toEqual(["x", "y"]);
  });
});

describe("versionsOf and disagreements", () => {
  it("walks a line of versions from any member, newest first", () => {
    const edges = [edge("c", "b", "supersedes"), edge("b", "a", "supersedes")];
    expect(versionsOf("b", edges)).toEqual(["c", "b", "a"]);
    expect(versionsOf("a", edges)).toEqual(["c", "b", "a"]);
    expect(versionsOf("z", edges)).toEqual(["z"]);
  });

  it("lists each disagreeing pair once", () => {
    expect(disagreements(GRAPH.edges)).toEqual([["tea", "coffee"]]);
  });
});

describe("MemoryMap", () => {
  it("draws each line as a dot named by type and state, and says what is locked", async () => {
    api.getMemoryGraph.mockResolvedValue(GRAPH);
    renderMap();
    expect(await screen.findByRole("group", { name: "Memory map: 5 lines, 3 links" })).toBeVisible();
    expect(dots()).toEqual([
      "Preferences: Likes tea, waiting for your OK",
      "Preferences: Likes coffee only",
      "Facts: Has a 5090",
      "Facts: Has a 4090, replaced",
      `People: ${PRIVATE_LABEL}`,
    ]);
    expect(screen.getByText("2 private lines are locked, so not on the map.")).toBeVisible();
  });

  it("filters by type and by lines waiting for an OK", async () => {
    api.getMemoryGraph.mockResolvedValue(GRAPH);
    renderMap();
    await screen.findByRole("group", { name: /^Memory map/ });
    await userEvent.click(screen.getByRole("checkbox", { name: "Facts" }));
    expect(dots()).toEqual(["Preferences: Likes tea, waiting for your OK", "Preferences: Likes coffee only", `People: ${PRIVATE_LABEL}`]);
    await userEvent.click(screen.getByRole("checkbox", { name: "Only lines waiting for your OK" }));
    expect(dots()).toEqual(["Preferences: Likes tea, waiting for your OK"]);
    await userEvent.click(screen.getByRole("checkbox", { name: "Preferences" }));
    expect(screen.getByText(MAP_NONE_SHOWN)).toBeVisible();
  });

  it("picking a dot shows the list's own card and its line of versions", async () => {
    api.getMemoryGraph.mockResolvedValue(GRAPH);
    renderMap();
    await userEvent.click(await screen.findByRole("button", { name: "Facts: Has a 4090, replaced" }));
    const panel = within(screen.getByRole("group", { name: "Picked: Has a 4090" }));
    // A line with no card in the list shows its words in a note.
    expect(panel.getByText("Has a 4090", { selector: "p" })).toBeVisible();
    await userEvent.click(panel.getByRole("button", { name: "Has a 5090" }));
    const newer = within(screen.getByRole("group", { name: "Picked: Has a 5090" }));
    expect(newer.getByRole("listitem", { name: "Card Has a 5090" })).toBeVisible();
    expect(within(newer.getByRole("list", { name: "Versions" })).getAllByRole("listitem")).toHaveLength(2);
  });

  it("opens a dot from the keyboard, and a private one keeps its words closed", async () => {
    api.getMemoryGraph.mockResolvedValue(GRAPH);
    renderMap();
    const secret = await screen.findByRole("button", { name: `People: ${PRIVATE_LABEL}` });
    fireEvent.keyDown(secret, { key: "Enter" });
    expect(secret).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByText(MAP_PRIVATE_NOTE)).toBeVisible();
  });

  it("settles a disagreement by keeping one line", async () => {
    api.getMemoryGraph.mockResolvedValue(GRAPH);
    const onKeep = renderMap();
    const box = within(await screen.findByRole("group", { name: "Lines that disagree" }));
    await userEvent.click(box.getByRole("button", { name: "Keep Likes coffee only, forget Likes tea" }));
    expect(onKeep).toHaveBeenCalledWith("coffee", "tea");
  });

  it("offers the map as a list with each line's links", async () => {
    api.getMemoryGraph.mockResolvedValue(GRAPH);
    renderMap();
    await screen.findByRole("group", { name: /^Memory map/ });
    expect(screen.getByText("The map as a list")).toBeVisible();
    const facts = within(screen.getByRole("list", { name: "Facts", hidden: true }));
    expect(facts.getByText("· Replaces Has a 4090")).toBeInTheDocument();
  });

  it("reloads when the rows change, and says when there is nothing to map", async () => {
    api.getMemoryGraph.mockResolvedValue({ nodes: [], edges: [], locked: 0, truncated: false });
    const { rerender } = render(<MemoryMap rows={[]} renderRow={() => null} onKeep={vi.fn()} />);
    expect(await screen.findByText(MAP_EMPTY)).toBeVisible();
    api.getMemoryGraph.mockResolvedValue(GRAPH);
    rerender(<MemoryMap rows={ROWS} renderRow={() => null} onKeep={vi.fn()} />);
    expect(await screen.findByRole("group", { name: /^Memory map/ })).toBeVisible();
    expect(api.getMemoryGraph).toHaveBeenCalledTimes(2);
  });

  it("says why when the map can't load", async () => {
    api.getMemoryGraph.mockRejectedValue(new Error("memory store is off"));
    renderMap();
    expect(await screen.findByText("The map couldn't load: memory store is off")).toBeVisible();
  });
});

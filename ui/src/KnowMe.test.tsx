import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { KnowMeAnswer, KnowMeView } from "./knowMeApi";

const api = vi.hoisted(() => ({
  getKnowMe: vi.fn<() => Promise<KnowMeView>>(),
  setKnowMeOn: vi.fn<(on: boolean) => Promise<KnowMeAnswer>>(),
  setKnowMeSignal: vi.fn<(id: string, on: boolean) => Promise<KnowMeAnswer>>(),
  knowMeScan: vi.fn<() => Promise<KnowMeAnswer>>(),
  knowMeDelete: vi.fn<(tag: string) => Promise<KnowMeAnswer>>(),
  knowMeWipe: vi.fn<() => Promise<KnowMeAnswer>>(),
}));
vi.mock("./knowMeApi", () => api);

import { KnowMe } from "./KnowMe";

const SIGNALS = [
  { id: "games", label: "Game library", reads: "Game names from Steam.", on: true },
  { id: "dev", label: "Dev tools", reads: "Tool names on PATH.", on: false },
];

function view(over: Partial<KnowMeView> = {}): KnowMeView {
  return {
    on: true,
    signals: SIGNALS,
    lastScan: "Last scan 1m ago: looked at game library 3.",
    tags: [
      { tag: "Stardew Valley", from: "from game library" },
      { tag: "Rust", from: "from dev tools" },
    ],
    ...over,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  api.getKnowMe.mockResolvedValue(view());
});

describe("KnowMe", () => {
  it("shows only the switch while off", async () => {
    api.getKnowMe.mockResolvedValue(view({ on: false, lastScan: null, tags: [] }));
    render(<KnowMe />);
    const sw = await screen.findByRole("switch", { name: "Know me from this computer" });
    expect(sw).not.toBeChecked();
    expect(screen.queryByRole("button", { name: "Scan now" })).toBeNull();
    expect(screen.queryByText("What I know about you")).toBeNull();
  });

  it("turns on and shows what the first scan found", async () => {
    api.getKnowMe.mockResolvedValue(view({ on: false, lastScan: null, tags: [] }));
    api.setKnowMeOn.mockResolvedValue({ view: view(), status: "" });
    render(<KnowMe />);
    const sw = await screen.findByRole("switch", { name: "Know me from this computer" });
    await userEvent.click(sw);
    expect(api.setKnowMeOn).toHaveBeenCalledWith(true);
    expect(await screen.findByText("Stardew Valley")).toBeInTheDocument();
  });

  it("lists signals, the last scan and each tag with where it came from", async () => {
    render(<KnowMe />);
    const signals = within(await screen.findByRole("list", { name: "What it looks at" }));
    expect(signals.getByRole("switch", { name: "Game library" })).toBeChecked();
    expect(signals.getByRole("switch", { name: "Dev tools" })).not.toBeChecked();
    expect(screen.getByText("Last scan 1m ago: looked at game library 3.")).toBeInTheDocument();
    const rust = within(screen.getByRole("listitem", { name: "Rust" }));
    expect(rust.getByText("from dev tools")).toBeInTheDocument();
    api.setKnowMeSignal.mockResolvedValue({ view: view(), status: "" });
    await userEvent.click(signals.getByRole("switch", { name: "Dev tools" }));
    expect(api.setKnowMeSignal).toHaveBeenCalledWith("dev", true);
  });

  it("deletes a tag only after asking", async () => {
    api.knowMeDelete.mockResolvedValue({
      view: view({ tags: [{ tag: "Rust", from: "from dev tools" }] }),
      status: "",
    });
    render(<KnowMe />);
    const row = within(await screen.findByRole("listitem", { name: "Stardew Valley" }));
    await userEvent.click(row.getByRole("button", { name: "Delete" }));
    expect(api.knowMeDelete).not.toHaveBeenCalled();
    const ask = within(row.getByRole("group", { name: "Delete Stardew Valley?" }));
    await userEvent.click(ask.getByRole("button", { name: "Delete" }));
    expect(api.knowMeDelete).toHaveBeenCalledWith("Stardew Valley");
    expect(await screen.findByRole("listitem", { name: "Rust" })).toBeInTheDocument();
    expect(screen.queryByRole("listitem", { name: "Stardew Valley" })).toBeNull();
  });

  it("wipes all only after asking and shows the line", async () => {
    api.knowMeWipe.mockResolvedValue({
      view: view({ on: false, lastScan: null, tags: [] }),
      status: "Know me is off and everything it learned is gone.",
    });
    render(<KnowMe />);
    await userEvent.click(await screen.findByRole("button", { name: "Wipe all" }));
    expect(api.knowMeWipe).not.toHaveBeenCalled();
    const ask = within(screen.getByRole("group", { name: "Wipe everything Know me learned?" }));
    await userEvent.click(ask.getByRole("button", { name: "Wipe all" }));
    expect(api.knowMeWipe).toHaveBeenCalledTimes(1);
    expect(await screen.findByRole("status")).toHaveTextContent("Know me is off and everything it learned is gone.");
    expect(screen.getByRole("switch", { name: "Know me from this computer" })).not.toBeChecked();
  });
});

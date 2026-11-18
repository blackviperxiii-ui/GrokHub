import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { LiveView } from "./api";

const api = vi.hoisted(() => ({
  pulseLiveAllow: vi.fn<() => Promise<LiveView>>(),
  pulseLiveDeny: vi.fn<() => Promise<LiveView>>(),
  pulseLiveStop: vi.fn<() => Promise<LiveView>>(),
  pulseLiveUndo: vi.fn<() => Promise<LiveView>>(),
  pulseLiveClose: vi.fn<() => Promise<void>>(),
}));

vi.mock("./api", () => api);

import { ALLOW, DENY, LiveWindow, START_FIX, STOP_FIX, UNDO_FIX, markWords } from "./PulseLive";

function view(over: Partial<LiveView> = {}): LiveView {
  return {
    cardId: "live-1",
    label: "Run the fans at full speed",
    title: "GPU hit 87°C in the last session",
    summary: "Save each fan, set full speed, then check.",
    steps: [
      { plain: "Save each fan's setting.", mark: "pending" },
      { plain: "Set every fan to full speed.", mark: "pending" },
      { plain: "Check the fans read full speed.", mark: "pending" },
    ],
    phase: "ready",
    result: null,
    canUndo: false,
    primary: START_FIX,
    stop: null,
    ...over,
  };
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("PulseLive", () => {
  it("markWords covers every checklist state", () => {
    expect(markWords("pending")).toBe("not started");
    expect(markWords("running")).toBe("running");
    expect(markWords("needsOk")).toBe("waiting for your OK");
    expect(markWords("done")).toBe("done");
    expect(markWords("failed")).toBe("didn't work");
    expect(markWords("skipped")).toBe("didn't run");
  });

  it("opens a live checklist dialog and starts the fix", async () => {
    const onClose = vi.fn();
    api.pulseLiveAllow.mockResolvedValue(
      view({
        phase: "awaiting",
        primary: null,
        stop: STOP_FIX,
        steps: [
          { plain: "Save each fan's setting.", mark: "needsOk" },
          { plain: "Set every fan to full speed.", mark: "pending" },
          { plain: "Check the fans read full speed.", mark: "pending" },
        ],
      }),
    );
    render(<LiveWindow initial={view()} onClose={onClose} />);
    expect(screen.getByRole("dialog", { name: "Run the fans at full speed" })).toBeVisible();
    expect(screen.queryByText("waiting for your OK")).toBeNull();
    expect(screen.getAllByText("not started")).toHaveLength(3);
    await userEvent.click(screen.getByRole("button", { name: START_FIX }));
    expect(api.pulseLiveAllow).toHaveBeenCalled();
    expect(await screen.findByText("waiting for your OK")).toBeVisible();
    expect(screen.getByRole("button", { name: ALLOW })).toBeVisible();
    expect(screen.getByRole("button", { name: DENY })).toBeVisible();
    expect(screen.getByRole("button", { name: STOP_FIX })).toBeVisible();
  });

  it("Stop fix and Undo fix call the shell and Close dismisses", async () => {
    const onClose = vi.fn();
    api.pulseLiveStop.mockResolvedValue(
      view({
        phase: "stopped",
        primary: UNDO_FIX,
        stop: null,
        canUndo: true,
        result: "I stopped the fix.",
        steps: [
          { plain: "Save each fan's setting.", mark: "skipped" },
          { plain: "Set every fan to full speed.", mark: "skipped" },
          { plain: "Check the fans read full speed.", mark: "skipped" },
        ],
      }),
    );
    api.pulseLiveUndo.mockResolvedValue(
      view({
        phase: "stopped",
        primary: null,
        canUndo: false,
        result: "Undone.",
        steps: [
          { plain: "Save each fan's setting.", mark: "skipped" },
          { plain: "Set every fan to full speed.", mark: "skipped" },
          { plain: "Check the fans read full speed.", mark: "skipped" },
        ],
      }),
    );
    api.pulseLiveClose.mockResolvedValue();
    render(
      <LiveWindow
        initial={view({
          phase: "awaiting",
          primary: null,
          stop: STOP_FIX,
          steps: [
            { plain: "Save each fan's setting.", mark: "needsOk" },
            { plain: "Set every fan to full speed.", mark: "pending" },
            { plain: "Check the fans read full speed.", mark: "pending" },
          ],
        })}
        onClose={onClose}
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: STOP_FIX }));
    expect(api.pulseLiveStop).toHaveBeenCalled();
    expect(await screen.findByText("I stopped the fix.")).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: UNDO_FIX }));
    expect(api.pulseLiveUndo).toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(api.pulseLiveClose).toHaveBeenCalled();
    expect(onClose).toHaveBeenCalled();
  });
});

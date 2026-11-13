import "@testing-library/jest-dom/vitest";
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { RecordStatus } from "./api";
import { RecordIndicator, STOP_TITLE } from "./RecordIndicator";

const IDLE: RecordStatus = { recording: null, listening: null, events: [] };
const LIVE: RecordStatus = {
  recording: { secs: 42, text: "● Recording 0:42 of 2:00" },
  listening: null,
  events: [],
};

afterEach(() => vi.useRealTimers());

describe("RecordIndicator", () => {
  it("shows nothing while nothing records", async () => {
    const poll = vi.fn().mockResolvedValue(IDLE);
    const { container } = render(<RecordIndicator onEvent={vi.fn()} poll={poll} />);
    await act(async () => {});
    expect(poll).toHaveBeenCalled();
    expect(container).toBeEmptyDOMElement();
  });

  it("shows the clock and Stop while recording, and Stop ends it", async () => {
    const poll = vi.fn().mockResolvedValue(LIVE);
    const stop = vi.fn().mockResolvedValue(true);
    render(<RecordIndicator onEvent={vi.fn()} poll={poll} stop={stop} />);
    expect(await screen.findByText("● Recording 0:42 of 2:00")).toBeVisible();
    const button = screen.getByRole("button", { name: "Stop" });
    expect(button).toHaveAttribute("title", STOP_TITLE);
    await userEvent.click(button);
    expect(stop).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "Stopping…" })).toBeDisabled();
  });

  it("keeps showing for as long as the recording runs", async () => {
    vi.useFakeTimers();
    const poll = vi
      .fn()
      .mockResolvedValueOnce(LIVE)
      .mockResolvedValueOnce({ ...LIVE, recording: { secs: 43, text: "● Recording 0:43 of 2:00" } })
      .mockResolvedValue(IDLE);
    const { container } = render(<RecordIndicator onEvent={vi.fn()} poll={poll} everyMs={1000} />);
    await act(async () => {});
    expect(screen.getByText("● Recording 0:42 of 2:00")).toBeInTheDocument();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(screen.getByText("● Recording 0:43 of 2:00")).toBeInTheDocument();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(container).toBeEmptyDOMElement();
  });

  it("shows the audio check's line while the microphone is open", async () => {
    const poll = vi.fn().mockResolvedValue({
      ...IDLE,
      listening: { secs: 2, text: "● Audio check: listening 0:02 of 0:05" },
    });
    render(<RecordIndicator onEvent={vi.fn()} poll={poll} />);
    expect(await screen.findByText("● Audio check: listening 0:02 of 0:05")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Stop" })).toBeNull();
  });

  it("reports live while a recording or an audio check runs, once per change", async () => {
    vi.useFakeTimers();
    const onLive = vi.fn();
    const poll = vi
      .fn()
      .mockResolvedValueOnce(IDLE)
      .mockResolvedValueOnce(LIVE)
      .mockResolvedValueOnce({ ...IDLE, listening: { secs: 1, text: "● Audio check: listening 0:01 of 0:05" } })
      .mockResolvedValue(IDLE);
    render(<RecordIndicator onEvent={vi.fn()} onLive={onLive} poll={poll} everyMs={1000} />);
    await act(async () => {});
    expect(onLive).not.toHaveBeenCalled();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(onLive.mock.calls).toEqual([[true]]);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(onLive.mock.calls).toEqual([[true]]);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(onLive.mock.calls).toEqual([[true], [false]]);
  });

  it("hands each finished run over once", async () => {
    const onEvent = vi.fn();
    const done = { kind: "record" as const, ok: true, text: "Screen recording 0:07: 4 stills saved" };
    const poll = vi.fn().mockResolvedValue({ ...IDLE, events: [done] });
    render(<RecordIndicator onEvent={onEvent} poll={poll} />);
    await act(async () => {});
    expect(onEvent).toHaveBeenCalledWith(done);
  });
});

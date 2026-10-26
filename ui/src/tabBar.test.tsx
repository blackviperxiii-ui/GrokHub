import "@testing-library/jest-dom/vitest";
import { act, fireEvent, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { NEAR_TOP, SHOWN, THRESHOLD, USER_WINDOW_MS, onScroll, useHidingBar } from "./tabBar";

describe("onScroll", () => {
  it("hides only after a sustained scroll down", () => {
    let bar = onScroll(SHOWN, 100, 10);
    expect(bar).toEqual({ hidden: false, run: 10 });
    bar = onScroll(bar, 110, 10);
    expect(bar).toEqual({ hidden: false, run: 20 });
    bar = onScroll(bar, 115, 5);
    expect(bar).toEqual({ hidden: true, run: 25 });
  });

  it("does not jitter when the direction flips back and forth", () => {
    let bar = SHOWN;
    for (const delta of [15, -15, 15, -15, 15]) bar = onScroll(bar, 200, delta);
    expect(bar).toEqual({ hidden: false, run: 15 });
  });

  it("shows again on a sustained scroll up", () => {
    let bar = { hidden: true, run: 40 };
    bar = onScroll(bar, 300, -10);
    expect(bar).toEqual({ hidden: true, run: -10 });
    bar = onScroll(bar, 280, -20);
    expect(bar).toEqual({ hidden: false, run: -30 });
  });

  it("always shows near the top", () => {
    expect(onScroll({ hidden: true, run: 90 }, NEAR_TOP, 50)).toEqual(SHOWN);
    expect(onScroll({ hidden: true, run: 90 }, 0, -3)).toEqual(SHOWN);
  });

  it("keeps the state when nothing moved", () => {
    expect(onScroll({ hidden: true, run: 30 }, 400, 0)).toEqual({ hidden: true, run: 30 });
  });
});

function Probe({ pinned }: { pinned: boolean }) {
  const away = useHidingBar(pinned);
  return (
    <div>
      <p data-testid="bar">{away ? "away" : "shown"}</p>
      <div data-testid="page" />
      <input aria-label="Message" />
      <nav className="topbar">
        <button type="button">Chat</button>
      </nav>
    </div>
  );
}

function phone(matches: boolean) {
  vi.stubGlobal(
    "matchMedia",
    vi.fn(() => ({ matches, addEventListener: () => {}, removeEventListener: () => {} })),
  );
}

/** The user swipes `page` so it ends `top` px down. */
function swipe(page: HTMLElement, top: number) {
  fireEvent.touchMove(page);
  page.scrollTop = top;
  fireEvent.scroll(page);
}

describe("useHidingBar", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });

  it("slides away on a phone when the user scrolls down, and back on scroll up", () => {
    phone(true);
    const view = render(<Probe pinned={false} />);
    const page = view.getByTestId("page");
    swipe(page, 50);
    swipe(page, 50 + THRESHOLD);
    expect(view.getByTestId("bar")).toHaveTextContent("away");
    swipe(page, 50);
    expect(view.getByTestId("bar")).toHaveTextContent("shown");
  });

  it("comes back when a field or the bar gets focus", () => {
    phone(true);
    const view = render(<Probe pinned={false} />);
    const page = view.getByTestId("page");
    swipe(page, 50);
    swipe(page, 100);
    expect(view.getByTestId("bar")).toHaveTextContent("away");
    act(() => view.getByLabelText("Message").focus());
    expect(view.getByTestId("bar")).toHaveTextContent("shown");
    swipe(page, 200);
    expect(view.getByTestId("bar")).toHaveTextContent("away");
    act(() => view.getByRole("button", { name: "Chat" }).focus());
    expect(view.getByTestId("bar")).toHaveTextContent("shown");
  });

  it("stays while a dialog is open", () => {
    phone(true);
    const view = render(<Probe pinned />);
    const page = view.getByTestId("page");
    swipe(page, 50);
    swipe(page, 200);
    expect(view.getByTestId("bar")).toHaveTextContent("shown");
  });

  it("ignores scrolling the user did not do, like a streaming reply", () => {
    phone(true);
    vi.useFakeTimers();
    const view = render(<Probe pinned={false} />);
    const page = view.getByTestId("page");
    vi.advanceTimersByTime(USER_WINDOW_MS + 1);
    page.scrollTop = 50;
    fireEvent.scroll(page);
    page.scrollTop = 400;
    fireEvent.scroll(page);
    expect(view.getByTestId("bar")).toHaveTextContent("shown");
  });

  it("never moves on wider screens", () => {
    phone(false);
    const view = render(<Probe pinned={false} />);
    const page = view.getByTestId("page");
    swipe(page, 50);
    swipe(page, 400);
    expect(view.getByTestId("bar")).toHaveTextContent("shown");
  });
});

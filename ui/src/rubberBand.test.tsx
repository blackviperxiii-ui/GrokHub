import "@testing-library/jest-dom/vitest";
import { fireEvent, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { MAX_PULL, overscroll, rubberBand, useOverscroll } from "./rubberBand";

describe("rubberBand", () => {
  it("maps no pull to no offset", () => {
    expect(rubberBand(0)).toBe(0);
  });

  it("follows the formula with the default stiffness", () => {
    // (1 - 1 / (100 * 0.55 / 96 + 1)) * 96
    expect(rubberBand(100)).toBeCloseTo(34.967, 3);
    expect(rubberBand(96, 96, 1)).toBe(48);
  });

  it("grows with the pull but never reaches the max", () => {
    let last = 0;
    for (const x of [1, 10, 50, 200, 1_000, 100_000]) {
      const y = rubberBand(x);
      expect(y).toBeGreaterThan(last);
      expect(y).toBeLessThan(MAX_PULL);
      last = y;
    }
  });

  it("is stiffer with a lower c", () => {
    expect(rubberBand(100, 96, 0.3)).toBeLessThan(rubberBand(100, 96, 0.55));
    expect(rubberBand(100, 96, 0.8)).toBeGreaterThan(rubberBand(100, 96, 0.55));
  });

  it("mirrors negative pulls", () => {
    expect(rubberBand(-100)).toBeCloseTo(-34.967, 3);
  });
});

describe("overscroll", () => {
  it("stretches only past an edge", () => {
    expect(overscroll(100, 0, 500)).toBeCloseTo(34.967, 3);
    expect(overscroll(-100, 500, 500)).toBeCloseTo(-34.967, 3);
    expect(overscroll(100, 200, 500)).toBe(0);
    expect(overscroll(-100, 0, 500)).toBe(0);
  });
});

function Page() {
  useOverscroll();
  return <div className="log" data-testid="log" />;
}

function reduced(matches: boolean) {
  vi.stubGlobal(
    "matchMedia",
    vi.fn(() => ({ matches, addEventListener: () => {}, removeEventListener: () => {} })),
  );
}

function pull(log: HTMLElement, from: number, to: number) {
  fireEvent.touchStart(log, { touches: [{ clientY: from }] });
  fireEvent.touchMove(log, { touches: [{ clientY: to }] });
}

describe("useOverscroll", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("stretches a view pulled down at its top and snaps back on release", () => {
    reduced(false);
    const { getByTestId } = render(<Page />);
    const log = getByTestId("log");
    pull(log, 0, 100);
    expect(log.style.transform).toBe(`translateY(${rubberBand(100)}px)`);
    fireEvent.touchEnd(log);
    expect(log.style.transform).toBe("");
    expect(log).toHaveClass("snapping");
  });

  it("does not stretch with reduced motion", () => {
    reduced(true);
    const { getByTestId } = render(<Page />);
    const log = getByTestId("log");
    pull(log, 0, 100);
    expect(log.style.transform).toBe("");
    fireEvent.touchEnd(log);
    expect(log).not.toHaveClass("snapping");
  });
});

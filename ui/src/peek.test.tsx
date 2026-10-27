import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { onArrowFocus, peekExtent, revealScroll, type Span } from "./peek";

/** `n` rows of `size` px with `gap` px between them. */
const rows = (n: number, size: number, gap: number): Span[] =>
  Array.from({ length: n }, (_, i) => ({ start: i * (size + gap), end: i * (size + gap) + size }));

describe("peekExtent", () => {
  it("leaves a list that fits alone", () => {
    expect(peekExtent(rows(3, 36, 4), 200)).toBeNull();
    expect(peekExtent([], 200)).toBeNull();
  });

  it("cuts a long column halfway into the next row", () => {
    // Rows at 0, 40, 80, 120, 160: row 4 shows 18 of its 36 px.
    expect(peekExtent(rows(10, 36, 4), 200)).toBeCloseTo(178, 5);
    expect(peekExtent(rows(10, 36, 4), 200, 0.4, 0.4)).toBeCloseTo(174.4, 5);
  });

  it("keeps the room when it already cuts a chip in the band", () => {
    const chips: Span[] = [
      { start: 0, end: 120 },
      { start: 128, end: 200 },
      { start: 208, end: 408 },
    ];
    // 300 px shows 46% of the wide third chip: nothing to trim.
    expect(peekExtent(chips, 300)).toBe(300);
    // 260 px would show only 26% of it, so the second chip peeks instead.
    expect(peekExtent(chips, 260)).toBeCloseTo(164, 5);
  });

  it("falls back to the full room when not even the second item can peek", () => {
    expect(peekExtent(rows(3, 200, 0), 50)).toBe(50);
  });
});

describe("revealScroll", () => {
  const list = rows(10, 36, 4); // ends at 36, 76, 116, 156, ...

  it("stays put while the row and both neighbours show", () => {
    expect(revealScroll(0, 200, list, 2, 1)).toBe(0);
  });

  it("moving down scrolls the row after the selection into view early", () => {
    // Selection on row 4 (160-196); row 5 ends at 236.
    expect(revealScroll(0, 200, list, 4, 1)).toBe(36);
  });

  it("moving up keeps the row before the selection in view", () => {
    // Selection on row 3; row 2 starts at 80.
    expect(revealScroll(120, 200, list, 3, -1)).toBe(80);
    expect(revealScroll(120, 200, list, 0, -1)).toBe(0);
  });

  it("a wrap to the last row lands at the bottom", () => {
    // Row 8 starts at 320, row 9 ends at 396.
    expect(revealScroll(0, 100, list, 9, -1)).toBe(296);
  });

  it("when three rows don't fit, the one ahead wins", () => {
    // Rows 1-3 span 40-156 in a 100 px view.
    expect(revealScroll(0, 100, list, 2, 1)).toBe(56);
    expect(revealScroll(200, 100, list, 2, -1)).toBe(40);
  });
});

describe("onArrowFocus", () => {
  afterEach(() => vi.restoreAllMocks());

  function Row() {
    return (
      <div role="group" aria-label="Chips" onKeyDown={(e) => onArrowFocus(e, "x")}>
        {["One", "Two", "Three", "Four"].map((label) => (
          <button key={label} type="button" data-i={label}>
            {label}
          </button>
        ))}
      </div>
    );
  }

  it("Right and Left move focus, reveal early, and stop at the ends", async () => {
    // Chips 100 px wide with 10 px gaps in a 150 px row.
    const at = (el: HTMLElement) => ["One", "Two", "Three", "Four"].indexOf(el.dataset.i ?? "");
    vi.spyOn(HTMLElement.prototype, "offsetLeft", "get").mockImplementation(function (this: HTMLElement) {
      return Math.max(at(this), 0) * 110;
    });
    vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(100);
    vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(150);
    render(<Row />);
    const group = screen.getByRole("group");
    const scrollTo = vi.fn();
    group.scrollTo = scrollTo as unknown as typeof group.scrollTo;

    screen.getByRole("button", { name: "One" }).focus();
    await userEvent.keyboard("{ArrowRight}");
    expect(screen.getByRole("button", { name: "Two" })).toHaveFocus();
    // Chip Three (220-320) must show: 320 - 150.
    expect(scrollTo).toHaveBeenLastCalledWith({ left: 170, behavior: "smooth" });

    await userEvent.keyboard("{ArrowRight}{ArrowRight}{ArrowRight}");
    expect(screen.getByRole("button", { name: "Four" })).toHaveFocus();
    await userEvent.keyboard("{ArrowLeft}");
    expect(screen.getByRole("button", { name: "Three" })).toHaveFocus();
    // Up and Down belong to a column, not this row.
    await userEvent.keyboard("{ArrowDown}");
    expect(screen.getByRole("button", { name: "Three" })).toHaveFocus();
  });
});

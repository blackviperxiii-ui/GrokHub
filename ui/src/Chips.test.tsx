import "@testing-library/jest-dom/vitest";
import { act, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { QuickChip } from "./api";

const api = vi.hoisted(() => ({
  quickChips: vi.fn<(surface: string, id?: string, draft?: string) => Promise<QuickChip[]>>(),
  chipOutcome: vi.fn<(surface: string, id: string, picked: boolean) => Promise<boolean>>(),
}));

vi.mock("./api", () => api);

import { ChipRow, chipSlotOf, chipTip, DRAFT_SETTLE_MS, useChipRow } from "./Chips";

const chip = (id: string, label: string, over: Partial<QuickChip> = {}): QuickChip => ({
  id,
  label,
  value: label,
  action: "ask",
  hint: "",
  primary: false,
  ...over,
});

const keys = (code: string, mods: Partial<{ altKey: boolean; ctrlKey: boolean; metaKey: boolean }> = {}) => ({
  altKey: false,
  ctrlKey: false,
  metaKey: false,
  code,
  ...mods,
});

describe("chipSlotOf", () => {
  it("reads Alt+1 to Alt+3 by the key, whatever symbol the layout types", () => {
    expect(chipSlotOf(keys("Digit1", { altKey: true }))).toBe(0);
    expect(chipSlotOf(keys("Digit3", { altKey: true }))).toBe(2);
    expect(chipSlotOf(keys("Digit4", { altKey: true }))).toBeNull();
    expect(chipSlotOf(keys("Digit1"))).toBeNull();
    expect(chipSlotOf(keys("Digit1", { altKey: true, ctrlKey: true }))).toBeNull();
    expect(chipSlotOf(keys("KeyA", { altKey: true }))).toBeNull();
  });
});

describe("chipTip", () => {
  it("says why the chip is here, else what it sends, and its key", () => {
    expect(chipTip(chip("a", "Plan", { hint: "You often start here" }), 0)).toBe(
      "Why this? You often start here  (Alt+1)",
    );
    expect(chipTip(chip("b", "Plan my day"), 1)).toBe("Plan my day  (Alt+2)");
  });
});

describe("ChipRow", () => {
  it("shows three chips at most, each with its ×, and none at all when empty", async () => {
    const onTap = vi.fn();
    const onHide = vi.fn();
    const chips = [chip("a", "One", { primary: true }), chip("b", "Two"), chip("c", "Three"), chip("d", "Four")];
    const { rerender } = render(<ChipRow chips={chips} onTap={onTap} onHide={onHide} />);
    expect(screen.getByRole("button", { name: "One" })).toHaveClass("quick-chip", "primary");
    expect(screen.queryByRole("button", { name: "Four" })).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Two" }));
    expect(onTap).toHaveBeenCalledWith(chips[1]);
    await userEvent.click(screen.getByRole("button", { name: "Hide Three" }));
    expect(onHide).toHaveBeenCalledWith(chips[2]);
    rerender(<ChipRow chips={[]} onTap={onTap} onHide={onHide} />);
    expect(screen.queryByRole("group", { name: "Suggestions" })).toBeNull();
  });

  it("Alt+N taps while the row has keys, and not while busy", () => {
    const onTap = vi.fn();
    const chips = [chip("a", "One"), chip("b", "Two")];
    const { rerender } = render(<ChipRow chips={chips} busy onTap={onTap} onHide={vi.fn()} />);
    fireEvent.keyDown(window, { code: "Digit2", altKey: true });
    expect(onTap).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "One" })).toBeDisabled();
    rerender(<ChipRow chips={chips} keys={false} onTap={onTap} onHide={vi.fn()} />);
    fireEvent.keyDown(window, { code: "Digit2", altKey: true });
    expect(onTap).not.toHaveBeenCalled();
    rerender(<ChipRow chips={chips} onTap={onTap} onHide={vi.fn()} />);
    fireEvent.keyDown(window, { code: "Digit2", altKey: true });
    expect(onTap).toHaveBeenCalledWith(chips[1]);
  });
});

function Harness({ draft = "", busy = false }: { draft?: string; busy?: boolean }) {
  const row = useChipRow({ surface: "composer", conversationId: "shell-1", draft, busy });
  return <ChipRow chips={row.chips} onTap={row.picked} onHide={row.hide} />;
}

describe("useChipRow", () => {
  beforeEach(() => {
    api.quickChips.mockReset().mockResolvedValue([chip("a", "One"), chip("b", "Two")]);
    api.chipOutcome.mockReset().mockResolvedValue(true);
  });

  it("asks once typing settles, and a tap or a hide teaches and asks again", async () => {
    vi.useFakeTimers();
    try {
      const { rerender } = render(<Harness draft="he" />);
      rerender(<Harness draft="hello" />);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(DRAFT_SETTLE_MS - 1);
      });
      expect(api.quickChips).not.toHaveBeenCalled();
      await act(async () => {
        await vi.advanceTimersByTimeAsync(1);
      });
      expect(api.quickChips).toHaveBeenCalledTimes(1);
      expect(api.quickChips).toHaveBeenCalledWith("composer", "shell-1", "hello", undefined);
    } finally {
      vi.useRealTimers();
    }
    fireEvent.click(await screen.findByRole("button", { name: "Hide One" }));
    expect(screen.queryByRole("button", { name: "One" })).toBeNull();
    expect(api.chipOutcome).toHaveBeenCalledWith("composer", "a", false);
    await vi.waitFor(() => expect(api.quickChips).toHaveBeenCalledTimes(2));
    fireEvent.click(await screen.findByRole("button", { name: "Two" }));
    expect(api.chipOutcome).toHaveBeenLastCalledWith("composer", "b", true);
    await vi.waitFor(() => expect(api.quickChips).toHaveBeenCalledTimes(3));
  });

  it("shows nothing and asks nothing while busy", async () => {
    render(<Harness busy />);
    await act(async () => {});
    expect(api.quickChips).not.toHaveBeenCalled();
    expect(screen.queryByRole("group")).toBeNull();
  });
});

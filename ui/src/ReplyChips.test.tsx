import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { ReplyChipData } from "./api";
import { ReplyChips } from "./ReplyChips";

const CHIPS: ReplyChipData[] = [
  { key: "plan:go", label: "Yes, go", act: "send", text: "Go ahead with the plan." },
  { key: "plan:change", label: "Change this", act: "prefill", text: "Change the plan: " },
  { key: "plan:dismiss", label: "Not now", act: "dismiss", text: "" },
];

describe("ReplyChips", () => {
  it("renders one button per chip and reports the pick", async () => {
    const onPick = vi.fn();
    render(<ReplyChips chips={CHIPS} onPick={onPick} />);
    expect(screen.getByRole("group", { name: "Suggested replies" })).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Yes, go" }));
    expect(onPick).toHaveBeenCalledWith(CHIPS[0]);
    await userEvent.click(screen.getByRole("button", { name: "Change this" }));
    expect(onPick).toHaveBeenCalledWith(CHIPS[1]);
    await userEvent.click(screen.getByRole("button", { name: "Not now" }));
    expect(onPick).toHaveBeenCalledWith(CHIPS[2]);
  });

  it("renders nothing with no chips", () => {
    const { container } = render(<ReplyChips chips={[]} onPick={() => {}} />);
    expect(container.firstChild).toBeNull();
  });
});

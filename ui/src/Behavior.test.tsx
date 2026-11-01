import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Behavior as Shape, BehaviorPage } from "./api";

const api = vi.hoisted(() => ({
  getBehavior: vi.fn<() => Promise<BehaviorPage>>(),
  saveBehavior: vi.fn<(closeToTray: boolean, behavior: Shape) => Promise<BehaviorPage>>(),
}));

vi.mock("./api", () => api);

import { Behavior, SCHEDULE_NOTE } from "./Behavior";

const OFF: Shape = {
  autocomplete: false,
  quietStart: "",
  quietEnd: "",
  dailyTokenBudget: 0,
  pauseOverBudget: false,
};

function page(over: Partial<Shape> = {}, closeToTray = true): BehaviorPage {
  return {
    closeToTray,
    behavior: { ...OFF, ...over },
    budgets: [
      { value: 0, label: "Off" },
      { value: 1000000, label: "1.0M tokens a day" },
    ],
  };
}

describe("Behavior", () => {
  beforeEach(() => {
    api.getBehavior.mockReset().mockResolvedValue(page());
    api.saveBehavior.mockReset();
  });

  it("starts with autocomplete, budget and quiet hours off, and says nothing waits yet", async () => {
    render(<Behavior tray />);
    expect(await screen.findByLabelText("Sentence autocomplete")).not.toBeChecked();
    expect(screen.getByLabelText("Daily token budget")).toHaveValue("0");
    expect(screen.getByLabelText("Quiet hours start")).toHaveValue("");
    expect(screen.getByLabelText("Quiet hours end")).toHaveValue("");
    expect(screen.queryByLabelText("Pause scheduled work over budget")).toBeNull();
    expect(screen.getByText(SCHEDULE_NOTE)).toBeInTheDocument();
  });

  it("each toggle saves at once", async () => {
    const onCloseToTray = vi.fn();
    api.saveBehavior.mockResolvedValueOnce(page({ autocomplete: true }));
    render(<Behavior tray onCloseToTray={onCloseToTray} />);
    await userEvent.click(await screen.findByLabelText("Sentence autocomplete"));
    expect(api.saveBehavior).toHaveBeenLastCalledWith(true, { ...OFF, autocomplete: true });
    expect(await screen.findByLabelText("Sentence autocomplete")).toBeChecked();

    api.saveBehavior.mockResolvedValueOnce(page({ autocomplete: true }, false));
    await userEvent.click(screen.getByLabelText(/keeps Loadout in the tray/));
    expect(api.saveBehavior).toHaveBeenLastCalledWith(false, { ...OFF, autocomplete: true });
    expect(onCloseToTray).toHaveBeenCalledWith(false);

    api.saveBehavior.mockResolvedValueOnce(page({ autocomplete: true, dailyTokenBudget: 1000000 }, false));
    await userEvent.selectOptions(screen.getByLabelText("Daily token budget"), "1000000");
    expect(api.saveBehavior).toHaveBeenLastCalledWith(false, {
      ...OFF,
      autocomplete: true,
      dailyTokenBudget: 1000000,
    });
    const pause = await screen.findByLabelText("Pause scheduled work over budget");
    expect(pause).not.toBeChecked();
    api.saveBehavior.mockResolvedValueOnce(
      page({ autocomplete: true, dailyTokenBudget: 1000000, pauseOverBudget: true }, false),
    );
    await userEvent.click(pause);
    expect(api.saveBehavior).toHaveBeenLastCalledWith(false, {
      ...OFF,
      autocomplete: true,
      dailyTokenBudget: 1000000,
      pauseOverBudget: true,
    });
  });

  it("saves quiet hours and shows a refusal", async () => {
    api.saveBehavior.mockRejectedValueOnce("Set both times, or clear both for no quiet hours.");
    render(<Behavior tray={false} />);
    await userEvent.type(await screen.findByLabelText("Quiet hours start"), "22:00");
    await userEvent.click(screen.getByRole("button", { name: "Save quiet hours" }));
    expect(api.saveBehavior).toHaveBeenLastCalledWith(true, { ...OFF, quietStart: "22:00", quietEnd: "" });
    expect(await screen.findByText("Set both times, or clear both for no quiet hours.")).toBeInTheDocument();

    api.saveBehavior.mockResolvedValueOnce(page({ quietStart: "22:00", quietEnd: "07:00" }));
    await userEvent.type(screen.getByLabelText("Quiet hours end"), "07:00");
    await userEvent.click(screen.getByRole("button", { name: "Save quiet hours" }));
    expect(api.saveBehavior).toHaveBeenLastCalledWith(true, { ...OFF, quietStart: "22:00", quietEnd: "07:00" });
    expect(await screen.findByText("Quiet hours saved.")).toBeInTheDocument();
    expect(screen.queryByLabelText(/in the tray/)).toBeNull();
  });

  it("the web app sees the choices with nothing to change them", async () => {
    api.getBehavior.mockResolvedValue(page({ quietStart: "22:00", quietEnd: "07:00", dailyTokenBudget: 1000000 }));
    render(<Behavior tray readOnly />);
    expect(await screen.findByText("22:00–07:00")).toBeInTheDocument();
    expect(screen.getByText("1.0M tokens a day")).toBeInTheDocument();
    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.queryByRole("switch")).toBeNull();
    expect(screen.queryByRole("combobox")).toBeNull();
  });
});

import { Activity } from "react";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Behavior as Shape, BehaviorPage } from "./api";

const api = vi.hoisted(() => ({
  getBehavior: vi.fn<() => Promise<BehaviorPage>>(),
  saveBehavior: vi.fn<(closeToTray: boolean, behavior: Shape) => Promise<BehaviorPage>>(),
}));

vi.mock("./api", () => api);

import { AUTOCOMPLETE_NOTE, Behavior, SCHEDULE_NOTE, SUGGESTIONS_NOTE } from "./Behavior";

const OFF: Shape = {
  autocomplete: false,
  suggestions: true,
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

/** The page as Settings holds it: mounted once, hidden while another tab shows. */
function Shown({ shown }: { shown: boolean }) {
  return (
    <Activity mode={shown ? "visible" : "hidden"}>
      <Behavior tray={false} />
    </Activity>
  );
}

/** Let every answer already on its way land before looking at the page. */
const settle = () => act(() => new Promise<void>((done) => setTimeout(done, 0)));

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
    expect(screen.getByText(AUTOCOMPLETE_NOTE)).toBeInTheDocument();
    expect(AUTOCOMPLETE_NOTE).not.toMatch(/doesn't show|don't show/i);
  });

  it("suggestions start on, and turning them off saves at once", async () => {
    api.saveBehavior.mockResolvedValueOnce(page({ suggestions: false }));
    render(<Behavior tray />);
    const toggle = await screen.findByLabelText("Suggestions");
    expect(toggle).toBeChecked();
    expect(screen.getByText(SUGGESTIONS_NOTE)).toBeInTheDocument();
    await userEvent.click(toggle);
    expect(api.saveBehavior).toHaveBeenLastCalledWith(true, { ...OFF, suggestions: false });
    expect(await screen.findByLabelText("Suggestions")).not.toBeChecked();
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

  it("keeps an unsaved quiet hour when the page shows again, and refreshes the one left alone", async () => {
    const view = render(<Shown shown />);
    fireEvent.change(await screen.findByLabelText("Quiet hours start"), { target: { value: "22:00" } });
    api.getBehavior.mockResolvedValue(page({ quietStart: "08:00", quietEnd: "09:00", autocomplete: true }));
    view.rerender(<Shown shown={false} />);
    view.rerender(<Shown shown />);
    await waitFor(() => expect(api.getBehavior).toHaveBeenCalledTimes(2));
    await settle();
    expect(screen.getByLabelText("Quiet hours start")).toHaveValue("22:00");
    expect(screen.getByLabelText("Quiet hours end")).toHaveValue("09:00");
    expect(screen.getByLabelText("Sentence autocomplete")).toBeChecked();
  });

  it("untouched quiet hours take the saved ones when the page shows again", async () => {
    const view = render(<Shown shown />);
    expect(await screen.findByLabelText("Quiet hours start")).toHaveValue("");
    api.getBehavior.mockResolvedValue(page({ quietStart: "08:00", quietEnd: "09:00" }));
    view.rerender(<Shown shown={false} />);
    view.rerender(<Shown shown />);
    await waitFor(() => expect(screen.getByLabelText("Quiet hours start")).toHaveValue("08:00"));
    expect(screen.getByLabelText("Quiet hours end")).toHaveValue("09:00");
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

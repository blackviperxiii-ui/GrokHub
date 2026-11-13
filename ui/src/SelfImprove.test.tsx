import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SelfImproveView } from "./selfImproveApi";

const api = vi.hoisted(() => ({
  getSelfImprove: vi.fn<() => Promise<SelfImproveView>>(),
  setSelfImproveQuiet: vi.fn<(on: boolean) => Promise<SelfImproveView>>(),
  selfImproveRevert: vi.fn<(version: string) => Promise<string>>(),
  selfImproveIssue: vi.fn<(title: string) => Promise<{ text: string; copied: string }>>(),
}));
vi.mock("./selfImproveApi", () => api);

import { HISTORY_EMPTY, SelfImprove } from "./SelfImprove";

function view(over: Partial<SelfImproveView> = {}): SelfImproveView {
  return {
    quiet: false,
    quietNote: null,
    history: [
      { title: "Kept: Use ctrl+s in deploy", hint: "+2 -1 lines · 2d ago", revert: "skill:deploy@cccccccccccc" },
      { title: "Trying: Use ctrl+s in save-page", hint: "+1 -0 lines · 1d ago", revert: null },
    ],
    domains: "Runs by area (runs on file):\nsystem · 4 runs · 75% worked · 6.5 steps · 50% checked",
    bars: [{ domain: "system", runs: 4 }],
    ideas: [{ title: "Retry the build", text: "Retry once on a flaky step.", at: 2 }],
    ...over,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  api.getSelfImprove.mockResolvedValue(view());
});

describe("SelfImprove", () => {
  it("lists the history with Revert only on the newest kept change", async () => {
    render(<SelfImprove />);
    const list = within(await screen.findByRole("list", { name: "Self-improvement history" }));
    expect(list.getByText("Kept: Use ctrl+s in deploy")).toBeInTheDocument();
    expect(list.getByText("Trying: Use ctrl+s in save-page")).toBeInTheDocument();
    expect(list.getAllByRole("button", { name: "Revert" })).toHaveLength(1);
    expect(screen.getByText(/system · 4 runs · 75% worked/)).toBeInTheDocument();
    expect(screen.getByRole("list", { name: "Runs by area" })).toHaveTextContent("system");
  });

  it("Revert asks first, then reverts through the ledger", async () => {
    const user = userEvent.setup();
    api.selfImproveRevert.mockResolvedValue("Reverted by you: Use ctrl+s in deploy");
    render(<SelfImprove />);
    const list = within(await screen.findByRole("list", { name: "Self-improvement history" }));
    await user.click(list.getByRole("button", { name: "Revert" }));
    expect(api.selfImproveRevert).not.toHaveBeenCalled();
    const ask = within(screen.getByRole("group", { name: "Revert Use ctrl+s in deploy?" }));
    await user.click(ask.getByRole("button", { name: "Keep" }));
    expect(api.selfImproveRevert).not.toHaveBeenCalled();
    await user.click(list.getByRole("button", { name: "Revert" }));
    await user.click(within(screen.getByRole("group", { name: "Revert Use ctrl+s in deploy?" })).getByRole("button", { name: "Revert" }));
    expect(api.selfImproveRevert).toHaveBeenCalledWith("skill:deploy@cccccccccccc");
    expect(await screen.findByText("Reverted by you: Use ctrl+s in deploy")).toBeInTheDocument();
    expect(api.getSelfImprove).toHaveBeenCalledTimes(2);
  });

  it("quiet mode saves and shows what it logged", async () => {
    const user = userEvent.setup();
    api.setSelfImproveQuiet.mockResolvedValue(
      view({ quiet: true, quietNote: "Nothing logged yet. The self-review runs on Sunday at dream time." }),
    );
    render(<SelfImprove />);
    await user.click(await screen.findByRole("switch", { name: "Quiet self-review" }));
    expect(api.setSelfImproveQuiet).toHaveBeenCalledWith(true);
    expect(await screen.findByText("Nothing logged yet. The self-review runs on Sunday at dream time.")).toBeInTheDocument();
  });

  it("copies a saved idea as an issue and sends nothing", async () => {
    const user = userEvent.setup();
    const write = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue();
    api.selfImproveIssue.mockResolvedValue({
      text: "## Retry the build\n\nRetry once on a flaky step.",
      copied: "Issue copied. Paste it on GitHub yourself; Loadout sent nothing.",
    });
    render(<SelfImprove />);
    const ideas = within(await screen.findByRole("list", { name: "Saved ideas" }));
    expect(ideas.getByText("Idea: Retry the build")).toBeInTheDocument();
    await user.click(ideas.getByRole("button", { name: "Copy as GitHub issue" }));
    expect(api.selfImproveIssue).toHaveBeenCalledWith("Retry the build");
    expect(write).toHaveBeenCalledWith("## Retry the build\n\nRetry once on a flaky step.");
    expect(await screen.findByText("Issue copied. Paste it on GitHub yourself; Loadout sent nothing.")).toBeInTheDocument();
  });

  it("says when nothing changed yet", async () => {
    api.getSelfImprove.mockResolvedValue(view({ history: [], bars: [], domains: null, ideas: [] }));
    render(<SelfImprove />);
    expect(await screen.findByText(HISTORY_EMPTY)).toBeInTheDocument();
  });
});

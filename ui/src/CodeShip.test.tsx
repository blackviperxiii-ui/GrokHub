import "@testing-library/jest-dom/vitest";
import { Activity } from "react";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CodeActivityEntry, CodeShipPlan } from "./api";

const api = vi.hoisted(() => ({
  codeShipPlan: vi.fn<(action: "push" | "pr") => Promise<CodeShipPlan>>(),
  codeShipRun: vi.fn<(action: "push" | "pr", confirmed: boolean, command?: string | null) => Promise<{ command: string; exit: number; output: string; summary: string }>>(),
  codeNewBranch: vi.fn<(name: string) => Promise<string>>(),
  codeActivity: vi.fn<() => Promise<CodeActivityEntry[]>>(),
}));

vi.mock("./api", () => api);

import { CodeShip, SHIP_CONFIRM_PR, SHIP_CONFIRM_PUSH, SHIP_NO_ACTIVITY, SHIP_RULES } from "./CodeShip";

function plan(action: "push" | "pr", over: Partial<CodeShipPlan> = {}): CodeShipPlan {
  return {
    action,
    branch: "code/fix",
    command:
      action === "push"
        ? "git push --porcelain origin HEAD:refs/heads/code/fix"
        : "gh pr create --base beta --head code/fix --fill",
    needsConfirm: true,
    blocked: null,
    mode: "ask",
    autoBase: "beta",
    appMade: true,
    prUrl: null,
    ...over,
  };
}

/** The section as the app holds it: mounted once, hidden while another tab shows. */
function Tab({ shown }: { shown: boolean }) {
  return (
    <Activity mode={shown ? "visible" : "hidden"}>
      <CodeShip />
    </Activity>
  );
}

/** Let every answer already on its way land before looking at the page. */
const settle = () => act(() => new Promise<void>((done) => setTimeout(done, 0)));

describe("Code tab → Push and PR", () => {
  beforeEach(() => {
    api.codeShipPlan.mockReset();
    api.codeShipRun.mockReset();
    api.codeNewBranch.mockReset();
    api.codeActivity.mockReset();
    api.codeActivity.mockResolvedValue([]);
    api.codeShipPlan.mockImplementation((action) => Promise.resolve(plan(action)));
    api.codeShipRun.mockResolvedValue({
      command: "git push --porcelain origin HEAD:refs/heads/code/fix",
      exit: 0,
      output: "To github.com:o/r.git\n",
      summary: "Pushed code/fix to origin.",
    });
  });

  it("in Ask mode every push shows the exact command first", async () => {
    render(<CodeShip />);
    expect(await screen.findByText(new RegExp(SHIP_RULES.replace(/[.]/g, "\\.")))).toBeInTheDocument();
    expect(screen.getByText(SHIP_NO_ACTIVITY)).toBeInTheDocument();
    for (let i = 0; i < 2; i++) {
      await userEvent.click(await screen.findByRole("button", { name: "Push code/fix" }));
      const card = await screen.findByRole("group", { name: SHIP_CONFIRM_PUSH });
      expect(card).toHaveTextContent("git push --porcelain origin HEAD:refs/heads/code/fix");
      expect(api.codeShipRun).toHaveBeenCalledTimes(i);
      await userEvent.click(screen.getByRole("button", { name: "Push" }));
      expect(api.codeShipRun).toHaveBeenLastCalledWith("push", true, "git push --porcelain origin HEAD:refs/heads/code/fix");
      expect(await screen.findByRole("status")).toHaveTextContent("Pushed code/fix to origin.");
    }
  });

  it("Auto-beta pushes an app branch without a confirm", async () => {
    api.codeShipPlan.mockImplementation((action) => Promise.resolve(plan(action, { mode: "autoBeta", needsConfirm: false })));
    render(<CodeShip />);
    expect(await screen.findByText(/Auto-beta: a branch made here pushes into beta without asking\./)).toBeInTheDocument();
    await userEvent.click(await screen.findByRole("button", { name: "Push code/fix" }));
    expect(api.codeShipRun).toHaveBeenCalledWith("push", false);
    expect(screen.queryByRole("group", { name: SHIP_CONFIRM_PUSH })).not.toBeInTheDocument();
  });

  it("a blocked push says why and runs nothing", async () => {
    const why = "main is the main or default branch. The app never pushes to it; make a branch first.";
    api.codeShipPlan.mockImplementation((action) => Promise.resolve(plan(action, { branch: "main", blocked: why })));
    render(<CodeShip />);
    await userEvent.click(await screen.findByRole("button", { name: "Push main" }));
    expect(await screen.findByRole("status")).toHaveTextContent(why);
    expect(api.codeShipRun).not.toHaveBeenCalled();
    expect(screen.queryByRole("group", { name: SHIP_CONFIRM_PUSH })).not.toBeInTheDocument();
  });

  it("Open PR into beta confirms the gh command", async () => {
    render(<CodeShip />);
    await userEvent.click(await screen.findByRole("button", { name: "Open PR into beta" }));
    const card = await screen.findByRole("group", { name: SHIP_CONFIRM_PR });
    expect(card).toHaveTextContent("gh pr create --base beta --head code/fix --fill");
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(api.codeShipRun).not.toHaveBeenCalled();
  });

  it("makes a branch and lists the activity log", async () => {
    api.codeNewBranch.mockResolvedValue("code/new");
    api.codeActivity.mockResolvedValue([
      {
        tsMs: 1_700_000_000_000,
        kind: "blocked",
        repoKey: "k",
        host: "github",
        branch: "main",
        command: "git push --porcelain origin HEAD:refs/heads/main",
        mode: "ask",
        approvedBy: "none",
        exit: null,
        summary: "main is the main or default branch.",
      },
    ]);
    render(<CodeShip />);
    const log = await screen.findByRole("list", { name: "Activity" });
    expect(log).toHaveTextContent("Blocked");
    expect(log).toHaveTextContent("git push --porcelain origin HEAD:refs/heads/main");
    await userEvent.type(screen.getByRole("textbox", { name: "New branch" }), "code/new");
    await userEvent.click(screen.getByRole("button", { name: "Make branch" }));
    expect(api.codeNewBranch).toHaveBeenCalledWith("code/new");
    expect(await screen.findByRole("status")).toHaveTextContent("On code/new now.");
  });

  it("puts the Activity list, which scrolls in its own box, in the tab order", async () => {
    api.codeActivity.mockResolvedValue([
      {
        tsMs: 1_700_000_000_000,
        kind: "push",
        repoKey: "k",
        host: "github",
        branch: "beta",
        command: "git push --porcelain origin HEAD:refs/heads/beta",
        mode: "ask",
        approvedBy: "user",
        exit: 0,
        summary: "Pushed beta.",
      },
    ]);
    render(<CodeShip />);
    const log = await screen.findByRole("list", { name: "Activity" });
    expect(log).toHaveClass("code-activity");
    expect(log).toHaveAttribute("tabindex", "0");
  });

  it("a confirm card goes away when the branch changes, and a confirm sends the command it showed", async () => {
    let branch = "code/a";
    api.codeShipPlan.mockImplementation((action) =>
      Promise.resolve(plan(action, { branch, command: `git push --porcelain origin HEAD:refs/heads/${branch}` })),
    );
    api.codeNewBranch.mockImplementation((name) => {
      branch = name;
      return Promise.resolve(name);
    });
    render(<CodeShip />);
    await userEvent.click(await screen.findByRole("button", { name: "Push code/a" }));
    expect(await screen.findByRole("group", { name: SHIP_CONFIRM_PUSH })).toHaveTextContent(
      "git push --porcelain origin HEAD:refs/heads/code/a",
    );
    await userEvent.type(screen.getByRole("textbox", { name: "New branch" }), "code/b");
    await userEvent.click(screen.getByRole("button", { name: "Make branch" }));
    expect(await screen.findByRole("button", { name: "Push code/b" })).toBeEnabled();
    expect(screen.queryByRole("group", { name: SHIP_CONFIRM_PUSH })).toBeNull();
    expect(api.codeShipRun).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Push code/b" }));
    expect(await screen.findByRole("group", { name: SHIP_CONFIRM_PUSH })).toHaveTextContent(
      "git push --porcelain origin HEAD:refs/heads/code/b",
    );
    await userEvent.click(screen.getByRole("button", { name: "Push" }));
    expect(api.codeShipRun).toHaveBeenCalledTimes(1);
    expect(api.codeShipRun).toHaveBeenLastCalledWith("push", true, "git push --porcelain origin HEAD:refs/heads/code/b");
  });

  it("when the tab shows again, a confirm card stays for the same command and goes for another branch's", async () => {
    let branch = "code/a";
    api.codeShipPlan.mockImplementation((action) =>
      Promise.resolve(plan(action, { branch, command: `git push --porcelain origin HEAD:refs/heads/${branch}` })),
    );
    const view = render(<Tab shown />);
    await userEvent.click(await screen.findByRole("button", { name: "Push code/a" }));
    expect(await screen.findByRole("group", { name: SHIP_CONFIRM_PUSH })).toHaveTextContent(
      "git push --porcelain origin HEAD:refs/heads/code/a",
    );
    expect(api.codeShipPlan).toHaveBeenCalledTimes(4);

    view.rerender(<Tab shown={false} />);
    view.rerender(<Tab shown />);
    await waitFor(() => expect(api.codeShipPlan).toHaveBeenCalledTimes(6));
    await settle();
    expect(screen.getByRole("group", { name: SHIP_CONFIRM_PUSH })).toHaveTextContent(
      "git push --porcelain origin HEAD:refs/heads/code/a",
    );

    branch = "code/b";
    view.rerender(<Tab shown={false} />);
    view.rerender(<Tab shown />);
    expect(await screen.findByRole("button", { name: "Push code/b" })).toBeEnabled();
    await settle();
    expect(screen.queryByRole("group", { name: SHIP_CONFIRM_PUSH })).toBeNull();
    expect(api.codeShipRun).not.toHaveBeenCalled();
  });

  it("making a branch closes an open confirm card at once, before the new plan is read", async () => {
    api.codeShipPlan.mockImplementation((action) =>
      Promise.resolve(plan(action, { branch: "code/a", command: "git push --porcelain origin HEAD:refs/heads/code/a" })),
    );
    api.codeNewBranch.mockResolvedValue("code/b");
    render(<CodeShip />);
    await userEvent.click(await screen.findByRole("button", { name: "Push code/a" }));
    expect(await screen.findByRole("group", { name: SHIP_CONFIRM_PUSH })).toHaveTextContent(
      "git push --porcelain origin HEAD:refs/heads/code/a",
    );
    // The plan for the new branch never answers in this test.
    api.codeShipPlan.mockImplementation(() => new Promise<CodeShipPlan>(() => {}));
    await userEvent.type(screen.getByRole("textbox", { name: "New branch" }), "code/b");
    await userEvent.click(screen.getByRole("button", { name: "Make branch" }));
    expect(await screen.findByRole("status")).toHaveTextContent("On code/b now.");
    expect(screen.queryByRole("group", { name: SHIP_CONFIRM_PUSH })).toBeNull();
    expect(api.codeShipRun).not.toHaveBeenCalled();
  });

  it("a new refresh key reads the plans and the activity list again and keeps everything else", async () => {
    api.codeShipPlan.mockImplementation((action) => Promise.resolve(plan(action, { mode: "autoBeta", needsConfirm: false })));
    let finish: (done: { command: string; exit: number; output: string; summary: string }) => void = () => {};
    api.codeShipRun.mockImplementationOnce(() => new Promise((done) => (finish = done)));
    const view = render(<CodeShip refreshKey={0} />);
    await userEvent.type(await screen.findByLabelText("New branch"), "code/keep-me");
    await userEvent.click(screen.getByRole("button", { name: "Push code/fix" }));
    expect(await screen.findByRole("button", { name: "Working…" })).toBeDisabled();
    // Mounting read the list once, and pressing Push read it again.
    expect(api.codeActivity).toHaveBeenCalledTimes(2);

    // A deploy finished while the push is still running.
    api.codeActivity.mockResolvedValue([
      {
        tsMs: 1_700_000_000_000,
        kind: "deploy",
        repoKey: "github.com/o/r",
        host: "github.com",
        branch: "code/fix",
        command: "npm run deploy",
        mode: "autoBeta",
        approvedBy: "remembered",
        exit: 0,
        summary: "Deploy finished.",
      },
    ]);
    view.rerender(<CodeShip refreshKey={1} />);
    expect(await screen.findByText("Deploy finished.")).toBeInTheDocument();
    expect(api.codeActivity).toHaveBeenCalledTimes(3);
    expect(screen.getByLabelText("New branch")).toHaveValue("code/keep-me");
    expect(screen.getByRole("button", { name: "Working…" })).toBeDisabled();
    expect(api.codeShipRun).toHaveBeenCalledTimes(1);

    await act(async () => finish({ command: "git push", exit: 0, output: "To github.com:o/r.git\n", summary: "Pushed code/fix to origin." }));
    expect(await screen.findByRole("status")).toHaveTextContent("Pushed code/fix to origin.");
    const output = screen.getByRole("log", { name: "Push output" });
    expect(output).toHaveTextContent("To github.com:o/r.git");
    expect(output).toHaveAttribute("tabindex", "0");
  });

  it("moves focus to Cancel when the confirm card appears", async () => {
    render(<CodeShip />);
    await userEvent.click(await screen.findByRole("button", { name: "Push code/fix" }));
    const card = await screen.findByRole("group", { name: SHIP_CONFIRM_PUSH });
    expect(document.activeElement).toBe(within(card).getByRole("button", { name: "Cancel" }));
  });
});

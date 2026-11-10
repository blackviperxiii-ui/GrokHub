import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CodeDeployPlan } from "./api";

const api = vi.hoisted(() => ({
  codeDeployPlan: vi.fn<() => Promise<CodeDeployPlan>>(),
  codeDeployRun: vi.fn<(command: string, confirmed: boolean) => Promise<{ command: string; exit: number; output: string; cut: boolean }>>(),
  onCodeDeployRun: vi.fn(() => Promise.resolve(() => {})),
}));

vi.mock("./api", () => api);

import { CodeDeploy, DEPLOY_CONFIRM, DEPLOY_OFF } from "./CodeDeploy";

function plan(over: Partial<CodeDeployPlan> = {}): CodeDeployPlan {
  return {
    deployEnabled: true,
    command: "npm run deploy -- --env staging",
    needsConfirm: true,
    remembered: null,
    mode: "ask",
    blocked: null,
    ...over,
  };
}

describe("Code tab → Deploy", () => {
  beforeEach(() => {
    api.codeDeployPlan.mockReset();
    api.codeDeployRun.mockReset();
    api.codeDeployPlan.mockResolvedValue(plan());
    api.codeDeployRun.mockResolvedValue({
      command: "npm run deploy -- --env staging",
      exit: 0,
      output: "token=[redacted]\ndone\n",
      cut: false,
    });
  });

  it("is off by default: the button is disabled and the note says where to turn it on", async () => {
    api.codeDeployPlan.mockResolvedValue(plan({ deployEnabled: false, command: "", blocked: DEPLOY_OFF }));
    render(<CodeDeploy />);
    expect(await screen.findByText(DEPLOY_OFF)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Deploy" })).toBeDisabled();
  });

  it("shows the exact command in a confirm card, and only runs after the person confirms", async () => {
    const done = vi.fn();
    render(<CodeDeploy onDeployed={done} />);
    await userEvent.click(await screen.findByRole("button", { name: "Deploy" }));
    const card = await screen.findByRole("group", { name: DEPLOY_CONFIRM });
    expect(card).toHaveTextContent("npm run deploy -- --env staging");
    expect(api.codeDeployRun).not.toHaveBeenCalled();
    await userEvent.click(within(card).getByRole("button", { name: "Deploy" }));
    expect(api.codeDeployRun).toHaveBeenCalledWith("npm run deploy -- --env staging", true);
    expect(await screen.findByText("Deploy finished.")).toBeInTheDocument();
    expect(screen.getByLabelText("Deploy output")).toHaveTextContent("token=[redacted]");
    expect(done).toHaveBeenCalledTimes(1);
  });

  it("cancel closes the card without running", async () => {
    render(<CodeDeploy />);
    await userEvent.click(await screen.findByRole("button", { name: "Deploy" }));
    await userEvent.click(await screen.findByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("group", { name: DEPLOY_CONFIRM })).toBeNull();
    expect(api.codeDeployRun).not.toHaveBeenCalled();
  });

  it("a remembered deploy in Auto-beta runs without a confirm card", async () => {
    api.codeDeployPlan.mockResolvedValue(plan({ mode: "autoBeta", needsConfirm: false, remembered: "npm run deploy -- --env staging" }));
    render(<CodeDeploy />);
    await userEvent.click(await screen.findByRole("button", { name: "Deploy" }));
    expect(await screen.findByText("Deploy finished.")).toBeInTheDocument();
    expect(api.codeDeployRun).toHaveBeenCalledWith("npm run deploy -- --env staging", false);
    expect(screen.queryByRole("group", { name: DEPLOY_CONFIRM })).toBeNull();
  });

  it("Ask mode asks on every deploy even when the command is remembered", async () => {
    api.codeDeployPlan.mockResolvedValue(plan({ remembered: "npm run deploy -- --env staging" }));
    render(<CodeDeploy />);
    for (let i = 0; i < 2; i++) {
      await userEvent.click(await screen.findByRole("button", { name: "Deploy" }));
      await screen.findByRole("group", { name: DEPLOY_CONFIRM });
      await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    }
    expect(api.codeDeployRun).not.toHaveBeenCalled();
  });

  it("a failed deploy shows its exit code", async () => {
    api.codeDeployPlan.mockResolvedValue(plan({ needsConfirm: false, mode: "autoBeta" }));
    api.codeDeployRun.mockResolvedValue({ command: "x", exit: 3, output: "boom\n", cut: false });
    render(<CodeDeploy />);
    await userEvent.click(await screen.findByRole("button", { name: "Deploy" }));
    expect(await screen.findByText("Deploy exited with 3.")).toBeInTheDocument();
  });
});

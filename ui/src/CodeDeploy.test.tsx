import "@testing-library/jest-dom/vitest";
import { Activity } from "react";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CodeDeployPlan } from "./api";

const api = vi.hoisted(() => ({
  codeDeployPlan: vi.fn<() => Promise<CodeDeployPlan>>(),
  codeDeployRun: vi.fn<(command: string, confirmed: boolean) => Promise<{ command: string; exit: number; output: string; cut: boolean }>>(),
  onCodeDeployRun: vi.fn(() => Promise.resolve(() => {})),
  codeRunStop: vi.fn<() => Promise<boolean>>(),
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
    api.codeRunStop.mockReset();
    api.codeRunStop.mockResolvedValue(true);
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

  /** The section as the app holds it: mounted once, hidden while another tab shows. */
  function Tab({ shown }: { shown: boolean }) {
    return (
      <Activity mode={shown ? "visible" : "hidden"}>
        <CodeDeploy />
      </Activity>
    );
  }

  it("coming back closes a confirm card whose command has changed in Settings", async () => {
    const view = render(<Tab shown />);
    await userEvent.click(await screen.findByRole("button", { name: "Deploy" }));
    expect(await screen.findByRole("group", { name: DEPLOY_CONFIRM })).toHaveTextContent("npm run deploy -- --env staging");
    api.codeDeployPlan.mockResolvedValue(plan({ command: "npm run deploy -- --env production" }));
    view.rerender(<Tab shown={false} />);
    view.rerender(<Tab shown />);
    await waitFor(() => expect(screen.queryByRole("group", { name: DEPLOY_CONFIRM })).toBeNull());
    expect(api.codeDeployRun).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Deploy" }));
    expect(await screen.findByRole("group", { name: DEPLOY_CONFIRM })).toHaveTextContent("npm run deploy -- --env production");
  });

  it("coming back closes a confirm card when deploy was turned off", async () => {
    const view = render(<Tab shown />);
    await userEvent.click(await screen.findByRole("button", { name: "Deploy" }));
    await screen.findByRole("group", { name: DEPLOY_CONFIRM });
    api.codeDeployPlan.mockResolvedValue(plan({ deployEnabled: false, command: "", blocked: DEPLOY_OFF }));
    view.rerender(<Tab shown={false} />);
    view.rerender(<Tab shown />);
    await waitFor(() => expect(screen.queryByRole("group", { name: DEPLOY_CONFIRM })).toBeNull());
    expect(screen.getByRole("button", { name: "Deploy" })).toBeDisabled();
    expect(screen.getByText(DEPLOY_OFF)).toBeInTheDocument();
  });

  it("coming back keeps a confirm card whose command is still the one that would run", async () => {
    const view = render(<Tab shown />);
    await userEvent.click(await screen.findByRole("button", { name: "Deploy" }));
    await screen.findByRole("group", { name: DEPLOY_CONFIRM });
    view.rerender(<Tab shown={false} />);
    view.rerender(<Tab shown />);
    await waitFor(() => expect(api.codeDeployPlan).toHaveBeenCalledTimes(3));
    await act(() => new Promise<void>((done) => setTimeout(done, 0)));
    expect(screen.getByRole("group", { name: DEPLOY_CONFIRM })).toHaveTextContent("npm run deploy -- --env staging");
  });

  it("moves focus to Cancel when the confirm card appears", async () => {
    render(<CodeDeploy />);
    await userEvent.click(await screen.findByRole("button", { name: "Deploy" }));
    const card = await screen.findByRole("group", { name: DEPLOY_CONFIRM });
    expect(document.activeElement).toBe(within(card).getByRole("button", { name: "Cancel" }));
  });

  it("a running deploy shows Stop beside it, and Stop ends the run", async () => {
    api.codeDeployPlan.mockResolvedValue(plan({ mode: "autoBeta", needsConfirm: false, remembered: "npm run deploy -- --env staging" }));
    let finish: (result: { command: string; exit: number; output: string; cut: boolean }) => void = () => {};
    api.codeDeployRun.mockImplementationOnce(() => new Promise((done) => (finish = done)));
    render(<CodeDeploy />);
    expect(screen.queryByRole("button", { name: "Stop" })).toBeNull();
    await userEvent.click(await screen.findByRole("button", { name: "Deploy" }));
    const running = await screen.findByRole("button", { name: "Deploying…" });
    expect(running).toBeDisabled();
    const stop = screen.getByRole("button", { name: "Stop" });
    expect(running.nextElementSibling).toBe(stop);
    await userEvent.click(stop);
    expect(api.codeRunStop).toHaveBeenCalledTimes(1);
    expect(api.codeRunStop).toHaveBeenCalledWith();
    await act(async () => finish({ command: "npm run deploy -- --env staging", exit: -1, output: "uploading\n", cut: false }));
    expect(await screen.findByText("Deploy stopped.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Stop" })).toBeNull();
    const output = screen.getByRole("log", { name: "Deploy output" });
    expect(output).toHaveTextContent("uploading");
    expect(output).toHaveAttribute("tabindex", "0");
  });

  it("says why a deploy could not be stopped", async () => {
    api.codeDeployPlan.mockResolvedValue(plan({ mode: "autoBeta", needsConfirm: false, remembered: "npm run deploy -- --env staging" }));
    api.codeDeployRun.mockImplementationOnce(() => new Promise(() => {}));
    api.codeRunStop.mockRejectedValue("The run could not be stopped.");
    render(<CodeDeploy />);
    await userEvent.click(await screen.findByRole("button", { name: "Deploy" }));
    await userEvent.click(await screen.findByRole("button", { name: "Stop" }));
    expect(await screen.findByRole("status")).toHaveTextContent("The run could not be stopped.");
    expect(screen.getByRole("button", { name: "Stop" })).toBeEnabled();
  });
});

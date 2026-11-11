import { Activity } from "react";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CODE_SETTINGS_NONE, CodeSettings, DEPLOY_SETTINGS_NONE, SHIP_MODE_ASK, SHIP_MODE_AUTO, SHIP_SETTINGS_RULES } from "./CodeSettings";

const api = vi.hoisted(() => ({
  codeTestRemembered: vi.fn<() => Promise<{ command: string | null }>>(),
  codeTestForget: vi.fn<() => Promise<{ command: string | null }>>(),
  getCodeShip: vi.fn(),
  saveCodeShip: vi.fn(),
  codeSecretList: vi.fn(() => Promise.resolve({ names: [] as string[] })),
  codeSecretSet: vi.fn(),
  codeSecretDelete: vi.fn(),
  codeDeployPlan: vi.fn(),
  codeDeploySave: vi.fn(),
  codeDeployForget: vi.fn(),
}));

vi.mock("./api", () => api);

const SHIP = {
  mode: "ask" as const,
  autoBase: "beta",
  hosts: { github: true, other: false },
  extraProtected: ["main"],
  activityKeep: 2000,
};

const DEPLOY_OFF = {
  deployEnabled: false,
  command: "",
  needsConfirm: true,
  remembered: null as string | null,
  mode: "ask" as const,
  blocked: "Deploy is off for this project. Turn it on in Settings → Code.",
};

/** The page as Settings holds it: mounted once, hidden while another tab shows. */
function Shown({ shown }: { shown: boolean }) {
  return (
    <Activity mode={shown ? "visible" : "hidden"}>
      <CodeSettings />
    </Activity>
  );
}

/** Let every answer already on its way land before looking at the page. */
const settle = () => act(() => new Promise<void>((done) => setTimeout(done, 0)));

describe("Settings → Code", () => {
  beforeEach(() => {
    api.codeTestRemembered.mockReset();
    api.codeTestForget.mockReset();
    api.getCodeShip.mockReset();
    api.codeDeployPlan.mockReset();
    api.codeDeploySave.mockReset();
    api.codeDeployForget.mockReset();
    api.codeDeployPlan.mockResolvedValue(DEPLOY_OFF);
    api.saveCodeShip.mockReset();
    api.getCodeShip.mockResolvedValue(SHIP);
    api.saveCodeShip.mockImplementation((ship: typeof SHIP) => Promise.resolve(ship));
  });

  it("shows the remembered command and forgets it when the toggle is turned off", async () => {
    api.codeTestRemembered.mockResolvedValue({ command: "cargo test" });
    api.codeTestForget.mockResolvedValue({ command: null });
    render(<CodeSettings />);
    expect(await screen.findByText(/Remembered command/)).toHaveTextContent("cargo test");
    const toggle = screen.getByRole("switch", { name: "Remember this project's test command" });
    expect(toggle).toHaveAttribute("aria-checked", "true");
    await userEvent.click(toggle);
    expect(api.codeTestForget).toHaveBeenCalledTimes(1);
    expect(await screen.findByText(CODE_SETTINGS_NONE)).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "Remember this project's test command" })).toHaveAttribute(
      "aria-checked",
      "false",
    );
  });

  it("leaves the forget toggle off and disabled when nothing is remembered", async () => {
    api.codeTestRemembered.mockResolvedValue({ command: null });
    render(<CodeSettings />);
    expect(await screen.findByText(CODE_SETTINGS_NONE)).toBeInTheDocument();
    const toggle = screen.getByRole("switch", { name: "Remember this project's test command" });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(toggle).toBeDisabled();
  });

  it("saves the push mode, PR base and protected branches", async () => {
    api.codeTestRemembered.mockResolvedValue({ command: null });
    render(<CodeSettings />);
    const ask = await screen.findByRole("radio", { name: SHIP_MODE_ASK });
    expect(ask).toBeChecked();
    expect(screen.getByText(SHIP_SETTINGS_RULES)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("radio", { name: SHIP_MODE_AUTO }));
    expect(api.saveCodeShip).toHaveBeenLastCalledWith({ ...SHIP, mode: "autoBeta" });
    expect(await screen.findByRole("radio", { name: SHIP_MODE_AUTO })).toBeChecked();
    const extra = screen.getByRole("textbox", { name: "Also protected" });
    await userEvent.clear(extra);
    await userEvent.type(extra, "main, release");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(api.saveCodeShip).toHaveBeenLastCalledWith({ ...SHIP, mode: "autoBeta", extraProtected: ["main", "release"] });
    const other = screen.getByRole("switch", { name: "Push to git hosts other than GitHub (always asks)" });
    expect(other).toHaveAttribute("aria-checked", "false");
    await userEvent.click(other);
    expect(api.saveCodeShip).toHaveBeenLastCalledWith({
      ...SHIP,
      mode: "autoBeta",
      extraProtected: ["main", "release"],
      hosts: { github: true, other: true },
    });
  });

  it("keeps a typed PR base and protected list when the page shows again", async () => {
    api.codeTestRemembered.mockResolvedValue({ command: null });
    const view = render(<Shown shown />);
    const base = await screen.findByRole("textbox", { name: "PR base" });
    await userEvent.clear(base);
    await userEvent.type(base, "develop");
    const extra = screen.getByRole("textbox", { name: "Also protected" });
    await userEvent.clear(extra);
    await userEvent.type(extra, "release");
    api.getCodeShip.mockResolvedValue({ ...SHIP, mode: "autoBeta", autoBase: "next", extraProtected: ["main", "stable"] });
    view.rerender(<Shown shown={false} />);
    view.rerender(<Shown shown />);
    await waitFor(() => expect(api.getCodeShip).toHaveBeenCalledTimes(2));
    await settle();
    expect(screen.getByRole("textbox", { name: "PR base" })).toHaveValue("develop");
    expect(screen.getByRole("textbox", { name: "Also protected" })).toHaveValue("release");
    expect(screen.getByRole("radio", { name: SHIP_MODE_AUTO })).toBeChecked();
  });

  it("an untouched PR base and protected list take the fresh values when the page shows again", async () => {
    api.codeTestRemembered.mockResolvedValue({ command: null });
    const view = render(<Shown shown />);
    expect(await screen.findByRole("textbox", { name: "PR base" })).toHaveValue("beta");
    expect(screen.getByRole("textbox", { name: "Also protected" })).toHaveValue("main");
    api.getCodeShip.mockResolvedValue({ ...SHIP, autoBase: "develop", extraProtected: ["main", "release"] });
    view.rerender(<Shown shown={false} />);
    view.rerender(<Shown shown />);
    await waitFor(() => expect(screen.getByRole("textbox", { name: "PR base" })).toHaveValue("develop"));
    expect(screen.getByRole("textbox", { name: "Also protected" })).toHaveValue("main, release");
  });

  it("keeps a typed deploy command when the page shows again", async () => {
    api.codeTestRemembered.mockResolvedValue({ command: null });
    const on = { ...DEPLOY_OFF, deployEnabled: true, command: "npm run deploy", blocked: null };
    api.codeDeployPlan.mockResolvedValue(on);
    const view = render(<Shown shown />);
    const box = await screen.findByRole("textbox", { name: "Deploy command" });
    expect(box).toHaveValue("npm run deploy");
    await userEvent.clear(box);
    await userEvent.type(box, "make deploy-staging");
    api.codeDeployPlan.mockResolvedValue({ ...on, command: "npm run release", remembered: "npm run release" });
    view.rerender(<Shown shown={false} />);
    view.rerender(<Shown shown />);
    await waitFor(() => expect(api.codeDeployPlan).toHaveBeenCalledTimes(2));
    await settle();
    expect(screen.getByRole("textbox", { name: "Deploy command" })).toHaveValue("make deploy-staging");
    expect(screen.getByText("npm run release")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save deploy command" })).toBeEnabled();
  });

  it("an untouched deploy command takes the fresh one when the page shows again", async () => {
    api.codeTestRemembered.mockResolvedValue({ command: null });
    const on = { ...DEPLOY_OFF, deployEnabled: true, command: "npm run deploy", blocked: null };
    api.codeDeployPlan.mockResolvedValue(on);
    const view = render(<Shown shown />);
    expect(await screen.findByRole("textbox", { name: "Deploy command" })).toHaveValue("npm run deploy");
    api.codeDeployPlan.mockResolvedValue({ ...on, command: "npm run release" });
    view.rerender(<Shown shown={false} />);
    view.rerender(<Shown shown />);
    await waitFor(() => expect(screen.getByRole("textbox", { name: "Deploy command" })).toHaveValue("npm run release"));
    expect(screen.getByRole("button", { name: "Save deploy command" })).toBeDisabled();
  });

  it("shows why a PR base was refused", async () => {
    api.codeTestRemembered.mockResolvedValue({ command: null });
    api.saveCodeShip.mockRejectedValue("The PR base can't be main: main is never pushed to.");
    render(<CodeSettings />);
    const base = await screen.findByRole("textbox", { name: "PR base" });
    await userEvent.clear(base);
    await userEvent.type(base, "main");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("The PR base can't be main: main is never pushed to.")).toBeInTheDocument();
  });
  it("deploy is off by default, and turning it on saves the typed command", async () => {
    api.codeTestRemembered.mockResolvedValue({ command: null });
    api.codeDeploySave.mockResolvedValue({ deployEnabled: true, command: "npm run deploy" });
    render(<CodeSettings />);
    const toggle = await screen.findByRole("switch", { name: "Let the Code tab deploy this project" });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    api.codeDeployPlan.mockResolvedValue({ ...DEPLOY_OFF, deployEnabled: true, command: "npm run deploy", blocked: null });
    await userEvent.type(screen.getByRole("textbox", { name: "Deploy command" }), "npm run deploy");
    await userEvent.click(toggle);
    expect(api.codeDeploySave).toHaveBeenLastCalledWith(true, "npm run deploy");
    expect(await screen.findByRole("switch", { name: "Let the Code tab deploy this project" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
  });

  it("shows the remembered deploy command and forgets it when the toggle is turned off", async () => {
    api.codeTestRemembered.mockResolvedValue({ command: null });
    const on = { ...DEPLOY_OFF, deployEnabled: true, command: "npm run deploy", blocked: null };
    api.codeDeployPlan.mockResolvedValue({ ...on, remembered: "npm run deploy" });
    api.codeDeployForget.mockResolvedValue({ ...on, remembered: null });
    render(<CodeSettings />);
    expect(await screen.findByText(/Remembered command/)).toHaveTextContent("Remembered command: npm run deploy");
    await userEvent.click(screen.getByRole("switch", { name: "Remember this project's deploy command" }));
    expect(api.codeDeployForget).toHaveBeenCalledTimes(1);
    expect(await screen.findByText(DEPLOY_SETTINGS_NONE)).toBeInTheDocument();
  });
});

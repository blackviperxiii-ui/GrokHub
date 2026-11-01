import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Capabilities, Settings as Shape } from "./api";

const api = vi.hoisted(() => ({
  getSettings: vi.fn<() => Promise<Shape>>(),
  saveDefaults: vi.fn<(workspace: string, approvals: string) => Promise<Shape>>(),
  saveSettings:
    vi.fn<
      (
        model: string,
        closeToTray: boolean,
        workspace: string,
        approvals: string,
        desktopControl: boolean,
      ) => Promise<Shape>
    >(),
  setApiKey: vi.fn<(key: string) => Promise<Shape>>(),
  clearApiKey: vi.fn<() => Promise<Shape>>(),
  setTheme: vi.fn<(theme: string) => Promise<Shape>>(),
}));

vi.mock("./api", () => api);
// Local AI has its own tests (LocalAi.test.tsx).
vi.mock("./LocalAi", () => ({ LocalAi: () => <p>Local AI panel</p> }));
vi.mock("./WebAccess", () => ({ WebAccess: () => <p>Web access panel</p> }));
vi.mock("./Labs", () => ({ Labs: () => <p>Labs panel</p> }));
vi.mock("./Personality", () => ({ Personality: () => <p>Personality panel</p> }));
vi.mock("./Playbooks", () => ({ Playbooks: () => <p>Playbooks panel</p> }));
vi.mock("./Interests", () => ({ Interests: () => <p>Interests panel</p> }));
vi.mock("./Permissions", () => ({ Permissions: () => <p>Permissions panel</p> }));
// Behavior, Privacy and Background have their own tests.
vi.mock("./Behavior", () => ({
  Behavior: ({ readOnly, onCloseToTray }: { readOnly?: boolean; onCloseToTray?: (on: boolean) => void }) =>
    readOnly ? (
      <p>Behavior read-only</p>
    ) : (
      <button type="button" onClick={() => onCloseToTray?.(false)}>
        Behavior panel
      </button>
    ),
}));
vi.mock("./Privacy", () => ({
  Privacy: ({ readOnly }: { readOnly?: boolean }) => <p>{readOnly ? "Privacy read-only" : "Privacy panel"}</p>,
}));
vi.mock("./Background", () => ({ Background: () => <p>Background panel</p> }));

import { ABOUT_NOTE, DESKTOP_NOTE, REMOTE_NOTE, Settings } from "./Settings";

async function openSection(name: string) {
  await userEvent.click(await screen.findByRole("button", { name }));
}

const DESKTOP: Capabilities = { desktopControl: true, pulse: true, localAi: true, tray: true, webAccess: true, plugins: true };
const PHONE: Capabilities = { desktopControl: false, pulse: false, localAi: false, tray: false, webAccess: false, plugins: false };
const WEB: Capabilities = { ...PHONE, remote: true };

function shape(over: Partial<Shape>): Shape {
  return {
    model: "grok-4.7",
    closeToTray: true,
    hasApiKey: false,
    workspace: "",
    workspacePath: "/home/user/GrokHub",
    approvals: "ask",
    desktopControl: false,
    theme: "system",
    version: "3.1.0",
    ...over,
  };
}

describe("Settings", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
    api.getSettings.mockResolvedValue(shape({}));
  });

  it("loads and saves the model, keeping what Defaults and Behavior saved", async () => {
    api.saveDefaults.mockResolvedValue(
      shape({ workspace: "/work", workspacePath: "/work", approvals: "always" }),
    );
    api.saveSettings.mockResolvedValue(shape({ model: "grok-5", workspace: "/work", approvals: "always" }));
    render(<Settings caps={DESKTOP} />);

    const model = await screen.findByLabelText("Model");
    expect(model).toHaveValue("grok-4.7");
    // The folder, approvals and tray moved out of General.
    expect(screen.queryByLabelText("Work folder")).toBeNull();
    expect(screen.queryByLabelText("Approvals")).toBeNull();
    expect(screen.queryByLabelText(/keeps Loadout in the tray/)).toBeNull();

    await openSection("Defaults");
    await userEvent.type(screen.getByLabelText("Work folder"), "/work");
    await userEvent.selectOptions(screen.getByLabelText("Approvals"), "always");
    await userEvent.click(screen.getByRole("button", { name: "Save defaults" }));
    expect(api.saveDefaults).toHaveBeenCalledWith("/work", "always");
    await openSection("Behavior");
    await userEvent.click(screen.getByRole("button", { name: "Behavior panel" }));

    await openSection("General");
    await userEvent.clear(screen.getByLabelText("Model"));
    await userEvent.type(screen.getByLabelText("Model"), " grok-5 ");
    await userEvent.click(screen.getByRole("button", { name: "Save settings" }));
    expect(api.saveSettings).toHaveBeenCalledWith("grok-5", false, "/work", "always", false);
    expect(await screen.findByText("Settings saved.")).toBeInTheDocument();
  });

  it("desktop control starts off, says what still asks, and saves", async () => {
    api.saveSettings.mockResolvedValue(shape({ desktopControl: true }));
    render(<Settings caps={DESKTOP} />);

    const desk = await screen.findByLabelText("Let Grok control the desktop");
    expect(desk).not.toBeChecked();
    expect(screen.queryByText(DESKTOP_NOTE)).not.toBeInTheDocument();
    await userEvent.click(desk);
    expect(screen.getByText(DESKTOP_NOTE)).toHaveClass("warn");
    await userEvent.click(screen.getByRole("button", { name: "Save settings" }));

    expect(api.saveSettings).toHaveBeenCalledWith("grok-4.7", true, "", "ask", true);
    expect(await screen.findByText("Settings saved.")).toBeInTheDocument();
    expect(screen.getByLabelText("Let Grok control the desktop")).toBeChecked();
  });

  it("saves a key without echoing it, then removes it", async () => {
    api.setApiKey.mockResolvedValue(shape({ hasApiKey: true }));
    api.clearApiKey.mockResolvedValue(shape({}));
    render(<Settings caps={DESKTOP} />);
    await openSection("Account");

    expect(await screen.findByText("No key saved yet.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Remove key" })).toBeNull();
    await userEvent.type(screen.getByLabelText("API key"), "sk-abcdefghijklmnopqrstuv");
    await userEvent.click(screen.getByRole("button", { name: "Save key" }));

    expect(api.setApiKey).toHaveBeenCalledWith("sk-abcdefghijklmnopqrstuv");
    expect(await screen.findByText("A key is saved.")).toBeInTheDocument();
    expect(screen.getByLabelText("API key")).toHaveValue("");

    await userEvent.click(screen.getByRole("button", { name: "Remove key" }));
    expect(api.clearApiKey).toHaveBeenCalledTimes(1);
    expect(await screen.findByText("No key saved yet.")).toBeInTheDocument();
  });

  it("shows a keychain error instead of hanging", async () => {
    api.setApiKey.mockRejectedValue("No system keychain is available.");
    render(<Settings caps={DESKTOP} />);
    await openSection("Account");
    await userEvent.type(await screen.findByLabelText("API key"), "sk-abcdefghijklmnopqrstuv");
    await userEvent.click(screen.getByRole("button", { name: "Save key" }));
    expect(await screen.findByText("No system keychain is available.")).toBeInTheDocument();
  });

  it("hides the desktop-only settings on a phone", async () => {
    api.getSettings.mockResolvedValue(shape({ desktopControl: true }));
    api.saveSettings.mockResolvedValue(shape({}));
    render(<Settings caps={PHONE} />);

    expect(await screen.findByLabelText("Model")).toBeInTheDocument();
    expect(screen.queryByLabelText("Let Grok control the desktop")).toBeNull();
    expect(screen.queryByText(DESKTOP_NOTE)).toBeNull();
    const nav = screen.getByRole("navigation", { name: "Settings sections" });
    expect(Array.from(nav.querySelectorAll("button"), (b) => b.textContent)).toEqual([
      "General",
      "Account",
      "Defaults",
      "Behavior",
      "Appearance",
      "Personality",
      "Playbooks",
      "Permissions",
      "Privacy & data",
      "Background",
      "About",
    ]);
  });

  it("shows them on a desktop", async () => {
    render(<Settings caps={DESKTOP} />);
    expect(await screen.findByLabelText("Let Grok control the desktop")).toBeInTheDocument();
    const nav = screen.getByRole("navigation", { name: "Settings sections" });
    expect(Array.from(nav.querySelectorAll("button"), (b) => b.textContent)).toEqual([
      "General",
      "Account",
      "Defaults",
      "Behavior",
      "Appearance",
      "Personality",
      "Playbooks",
      "Permissions",
      "Privacy & data",
      "Background",
      "Pulse",
      "Local AI",
      "Web access",
      "Labs",
      "About",
    ]);
  });

  it("opens every section from the nav, one at a time", async () => {
    render(<Settings caps={DESKTOP} />);
    expect(await screen.findByLabelText("Model")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "General" })).toHaveAttribute("aria-pressed", "true");

    await openSection("Account");
    expect(screen.getByLabelText("API key")).toBeInTheDocument();
    expect(screen.queryByLabelText("Model")).toBeNull();
    expect(screen.getByRole("button", { name: "Account" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "General" })).toHaveAttribute("aria-pressed", "false");

    await openSection("Defaults");
    expect(screen.getByLabelText("Work folder")).toBeInTheDocument();
    expect(screen.queryByLabelText("API key")).toBeNull();

    await openSection("Behavior");
    expect(screen.getByRole("button", { name: "Behavior panel" })).toBeInTheDocument();
    expect(screen.queryByLabelText("Work folder")).toBeNull();

    await openSection("Appearance");
    expect(screen.getByRole("button", { name: "System" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.queryByRole("button", { name: "Behavior panel" })).toBeNull();

    await openSection("Personality");
    expect(screen.getByText("Personality panel")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "System" })).toBeNull();

    await openSection("Playbooks");
    expect(screen.getByText("Playbooks panel")).toBeInTheDocument();
    expect(screen.queryByText("Personality panel")).toBeNull();

    await openSection("Permissions");
    expect(screen.getByText("Permissions panel")).toBeInTheDocument();
    expect(screen.queryByText("Playbooks panel")).toBeNull();

    await openSection("Privacy & data");
    expect(screen.getByText("Privacy panel")).toBeInTheDocument();
    expect(screen.queryByText("Permissions panel")).toBeNull();

    await openSection("Background");
    expect(screen.getByText("Background panel")).toBeInTheDocument();
    expect(screen.queryByText("Privacy panel")).toBeNull();

    await openSection("Pulse");
    expect(screen.getByText("Interests panel")).toBeInTheDocument();
    expect(screen.queryByText("Background panel")).toBeNull();

    await openSection("Local AI");
    expect(screen.getByText("Local AI panel")).toBeInTheDocument();
    expect(screen.queryByText("Interests panel")).toBeNull();

    await openSection("Web access");
    expect(screen.getByText("Web access panel")).toBeInTheDocument();
    expect(screen.queryByText("Local AI panel")).toBeNull();

    await openSection("Labs");
    expect(screen.getByText("Labs panel")).toBeInTheDocument();
    expect(screen.queryByText("Web access panel")).toBeNull();

    await openSection("About");
    expect(screen.getByText("Version 3.1.0")).toBeInTheDocument();
    expect(screen.getByText(ABOUT_NOTE)).toBeInTheDocument();
    expect(screen.queryByText("Labs panel")).toBeNull();
  });

  it("opens each section at the top and closes from the button or Esc", async () => {
    api.getSettings.mockResolvedValue(shape({}));
    const onClose = vi.fn();
    const { container } = render(<Settings caps={DESKTOP} onClose={onClose} />);
    await openSection("Playbooks");
    const pane = container.querySelector<HTMLElement>(".settings-pane")!;
    expect(screen.getByRole("heading", { level: 1, name: "Playbooks" })).toBeTruthy();
    pane.scrollTop = 400;
    await openSection("Permissions");
    expect(pane.scrollTop).toBe(0);
    expect(screen.getByRole("heading", { level: 1, name: "Permissions" })).toBeTruthy();

    await userEvent.click(screen.getByRole("button", { name: "Close settings" }));
    expect(onClose).toHaveBeenCalledTimes(1);
    await userEvent.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledTimes(2);
  });

  it("shows the web app the desktop's settings but nothing to change them with", async () => {
    api.getSettings.mockResolvedValue(shape({ hasApiKey: true, approvals: "auto" }));
    render(<Settings caps={WEB} />);

    expect(await screen.findByText(REMOTE_NOTE)).toBeInTheDocument();
    expect(screen.getByText("grok-4.7")).toBeInTheDocument();
    expect(screen.getByText("/home/user/GrokHub")).toBeInTheDocument();
    expect(screen.getByText("Auto")).toBeInTheDocument();
    expect(screen.getByText("Saved on your computer.")).toBeInTheDocument();
    expect(screen.queryByLabelText("API key")).toBeNull();
    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.queryByRole("textbox")).toBeNull();
    expect(screen.queryByText("Local AI panel")).toBeNull();
    expect(screen.queryByText("Web access panel")).toBeNull();
    // Defaults, Behavior, Privacy & data and Background, read-only.
    for (const head of ["Defaults", "Behavior", "Privacy & data", "Background"]) {
      expect(screen.getByRole("heading", { level: 2, name: head })).toBeInTheDocument();
    }
    expect(screen.getByText("Behavior read-only")).toBeInTheDocument();
    expect(screen.getByText("Privacy read-only")).toBeInTheDocument();
    expect(screen.getByText("Background panel")).toBeInTheDocument();
  });

  it("applies a theme at once and saves it", async () => {
    api.setTheme.mockResolvedValue(shape({ theme: "ops-dark" }));
    render(<Settings caps={DESKTOP} />);
    await openSection("Appearance");
    await userEvent.click(screen.getByRole("button", { name: "Ops Dark" }));
    expect(api.setTheme).toHaveBeenCalledWith("ops-dark");
    expect(document.documentElement.dataset.theme).toBe("ops-dark");
    expect(screen.getByRole("button", { name: "Ops Dark" })).toHaveAttribute("aria-pressed", "true");

    api.setTheme.mockResolvedValue(shape({ theme: "system" }));
    await userEvent.click(screen.getByRole("button", { name: "System" }));
    expect(document.documentElement.dataset.theme).toBeUndefined();
    expect(screen.getByRole("button", { name: "System" })).toHaveAttribute("aria-pressed", "true");
  });

  it("puts the theme back when saving it fails", async () => {
    api.setTheme.mockRejectedValue("disk full");
    render(<Settings caps={DESKTOP} />);
    await openSection("Appearance");
    await userEvent.click(screen.getByRole("button", { name: "Light" }));
    expect(await screen.findByText("disk full")).toBeVisible();
    expect(document.documentElement.dataset.theme).toBeUndefined();
    expect(screen.getByRole("button", { name: "System" })).toHaveAttribute("aria-pressed", "true");
  });
});

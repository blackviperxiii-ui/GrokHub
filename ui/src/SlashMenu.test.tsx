import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SessionMode, Settings, SlashEffect, SlashHit } from "./api";

const api = vi.hoisted(() => ({
  sendChat: vi.fn<(id: string, text: string, mode?: SessionMode) => Promise<string>>(),
  getSettings: vi.fn<() => Promise<Settings>>(),
  slashHits: vi.fn<(draft: string) => Promise<SlashHit[]>>(),
  runSlash: vi.fn<(line: string) => Promise<SlashEffect>>(),
  closeChat: vi.fn<(id: string) => Promise<void>>(),
}));

vi.mock("./api", () => ({
  sendChat: api.sendChat,
  getSettings: api.getSettings,
  slashHits: api.slashHits,
  runSlash: api.runSlash,
  closeChat: api.closeChat,
  answerElicit: vi.fn(),
  saveSettings: vi.fn(),
  answerPermission: vi.fn(),
  cancelChat: vi.fn(),
  setModel: vi.fn(),
  listModels: vi.fn(),
  // No chat saved from an earlier launch.
  getLastChat: () => Promise.resolve(null),
  loadChat: vi.fn(),
  onChatEvent: () => Promise.resolve(() => {}),
  onEventGap: () => Promise.resolve(() => {}),
}));

import { Chat } from "./Chat";
import { isSlashCommand } from "./SlashMenu";

const SETTINGS: Settings = {
  model: "grok-4.7",
  closeToTray: true,
  hasApiKey: true,
  workspace: "",
  workspacePath: "/home/me",
  approvals: "ask",
  desktopControl: false,
  theme: "system",
  version: "3.1.0",
  summonHotkey: false,
  notifications: true,
};

const ROWS: SlashHit[] = [
  { cmd: "/plan", hint: "Plan mode", insert: "/plan", runOnPick: true, disabled: null },
  { cmd: "/remember", hint: "Save a memory note…", insert: "/remember ", runOnPick: false, disabled: null },
  { cmd: "/diagnose", hint: "Read-only checks", insert: "/diagnose", runOnPick: false, disabled: "In the cabin" },
];

function rowsFor(draft: string): SlashHit[] {
  const t = draft.trim().toLowerCase();
  return ROWS.filter((r) => r.cmd.startsWith(t));
}

describe("composer slash commands", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.getSettings.mockResolvedValue(SETTINGS);
    api.slashHits.mockImplementation((draft) => Promise.resolve(rowsFor(draft)));
    api.sendChat.mockResolvedValue("end_turn");
    api.closeChat.mockResolvedValue();
  });

  it("tells commands from messages", () => {
    expect(isSlashCommand("/plan")).toBe(true);
    expect(isSlashCommand("  /remember ship it")).toBe(true);
    expect(isSlashCommand("/usr/bin is slow")).toBe(false);
    expect(isSlashCommand("/ hi")).toBe(false);
    expect(isSlashCommand("hello /plan")).toBe(false);
  });

  it("opens on / with the cabin's commands greyed, and Escape closes it", async () => {
    render(<Chat onOpenSettings={() => {}} />);
    const box = screen.getByLabelText("Message");
    await userEvent.type(box, "/");
    const list = await screen.findByRole("listbox", { name: "Commands" });
    expect(list).toHaveTextContent("/plan");
    expect(screen.getByRole("option", { name: /diagnose/ })).toHaveAttribute("aria-disabled", "true");
    expect(screen.getByText("In the cabin")).toBeInTheDocument();
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("listbox", { name: "Commands" })).not.toBeInTheDocument();
  });

  it("Tab fills in the pick, and Enter runs it and posts the result", async () => {
    api.runSlash.mockResolvedValue({ kind: "notice", text: "Remembered in workspace MEMORY.md" });
    render(<Chat onOpenSettings={() => {}} />);
    const box = screen.getByLabelText("Message");
    await userEvent.type(box, "/rem");
    await screen.findByRole("option", { name: /remember/ });
    await userEvent.keyboard("{Tab}");
    expect(box).toHaveValue("/remember ");
    await userEvent.type(box, "ship on fridays{Enter}");
    await waitFor(() => expect(api.runSlash).toHaveBeenCalledWith("/remember ship on fridays"));
    expect(await screen.findByRole("status", { name: "/remember result" })).toHaveTextContent(
      "Remembered in workspace MEMORY.md",
    );
    expect(box).toHaveValue("");
    expect(api.sendChat).not.toHaveBeenCalled();
  });

  it("Enter on a pick that takes no text runs it: /plan switches the mode", async () => {
    api.runSlash.mockResolvedValue({ kind: "mode", mode: "plan", text: null });
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "/pl");
    await screen.findByRole("option", { name: /plan/ });
    await userEvent.keyboard("{Enter}");
    await waitFor(() => expect(api.runSlash).toHaveBeenCalledWith("/plan"));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: /Plan/ })).toHaveAttribute("aria-pressed", "true"),
    );
    expect(api.sendChat).not.toHaveBeenCalled();
  });

  it("/btw with text sends the text in that mode", async () => {
    api.runSlash.mockResolvedValue({ kind: "mode", mode: "btw", text: "what does this do" });
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "/btw what does this do{Enter}");
    await waitFor(() =>
      expect(api.sendChat).toHaveBeenCalledWith(expect.any(String), "what does this do", "btw"),
    );
  });

  it("an unknown command shows the error and nothing is sent", async () => {
    api.runSlash.mockRejectedValue("Unknown command /nope. Type /help for the list.");
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "/nope{Enter}");
    expect(await screen.findByRole("alert")).toHaveTextContent("Unknown command /nope");
    expect(api.sendChat).not.toHaveBeenCalled();
  });

  it("/connectors opens its page", async () => {
    api.runSlash.mockResolvedValue({ kind: "open", view: "connectors" });
    const open = vi.fn();
    render(<Chat onOpenSettings={() => {}} onOpenView={open} />);
    await userEvent.type(screen.getByLabelText("Message"), "/connectors{Enter}");
    await waitFor(() => expect(open).toHaveBeenCalledWith("connectors"));
  });

  it("a normal message still sends, paths included", async () => {
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "/usr/bin is slow{Enter}");
    await waitFor(() =>
      expect(api.sendChat).toHaveBeenCalledWith(expect.any(String), "/usr/bin is slow", "chat"),
    );
    expect(api.runSlash).not.toHaveBeenCalled();
  });
});

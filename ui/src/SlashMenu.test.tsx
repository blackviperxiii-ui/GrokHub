import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SessionMode, Settings, SlashEffect, SlashHit } from "./api";

const api = vi.hoisted(() => ({
  sendChat: vi.fn<(id: string, text: string, mode?: SessionMode) => Promise<string>>(),
  getSettings: vi.fn<() => Promise<Settings>>(),
  slashHits: vi.fn<(draft: string) => Promise<SlashHit[]>>(),
  runSlash: vi.fn<(line: string) => Promise<SlashEffect>>(),
  closeChat: vi.fn<(id: string) => Promise<void>>(),
  addAutomation: vi.fn<(text: string) => Promise<unknown[]>>(),
}));

vi.mock("./api", async () => {
  const { chatTabApiStubs } = await import("./chatTabApiStubs");
  return {
  ...chatTabApiStubs(),
  quickChips: () => Promise.resolve([]),
  chipOutcome: () => Promise.resolve(true),
  sendChat: api.sendChat,
  getSettings: api.getSettings,
  slashHits: api.slashHits,
  runSlash: api.runSlash,
  closeChat: api.closeChat,
  addAutomation: api.addAutomation,
  answerElicit: vi.fn(),
  saveSettings: vi.fn(),
  answerPermission: vi.fn(),
  cancelChat: vi.fn(),
  // No chat saved from an earlier launch.
  getLastChat: () => Promise.resolve(null),
  loadChat: vi.fn(),
  onChatEvent: () => Promise.resolve(() => {}),
  helperQuestions: () => Promise.resolve([]),
  onEventGap: () => Promise.resolve(() => {}),
  // Composer attachments have their own tests (AttachChips.test.tsx).
  onFileDrop: () => Promise.resolve(() => {}),
  stageAttachment: vi.fn(),
  stageAttachmentPaths: vi.fn(),
  pickAttachments: vi.fn(),
  removeAttachment: vi.fn(),
};
});

import { Chat, NO_PLAN, PLAN_NEXT, UNDO_NA } from "./Chat";
import { isSlashCommand } from "./SlashMenu";

const SETTINGS: Settings = {
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
  { cmd: "/delete", hint: "Delete this chat (asks first)", insert: "/delete", runOnPick: true, disabled: null },
  { cmd: "/rename", hint: "Rename this chat…", insert: "/rename ", runOnPick: false, disabled: null },
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
    await waitFor(() => expect(api.runSlash).toHaveBeenCalledWith("/remember ship on fridays", expect.stringMatching(/^shell-/)));
    expect(await screen.findByRole("status", { name: "/remember result" })).toHaveTextContent(
      "Remembered in workspace MEMORY.md",
    );
    expect(box).toHaveValue("");
    expect(api.sendChat).not.toHaveBeenCalled();
  });

  it("Enter on a pick that takes no text runs it: /plan plans the next message", async () => {
    api.runSlash.mockResolvedValue({ kind: "mode", mode: "plan", text: null });
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "/pl");
    await screen.findByRole("option", { name: /plan/ });
    await userEvent.keyboard("{Enter}");
    await waitFor(() => expect(api.runSlash).toHaveBeenCalledWith("/plan", expect.stringMatching(/^shell-/)));
    expect(await screen.findByText(PLAN_NEXT)).toBeVisible();
    expect(api.sendChat).not.toHaveBeenCalled();
  });

  it("/btw with text while nothing runs just answers it", async () => {
    api.runSlash.mockResolvedValue({ kind: "mode", mode: "btw", text: "what does this do" });
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "/btw what does this do{Enter}");
    await waitFor(() =>
      expect(api.sendChat).toHaveBeenCalledWith(expect.any(String), "what does this do", "chat"),
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
      expect(api.sendChat).toHaveBeenCalledWith(expect.any(String), "/usr/bin is slow", undefined),
    );
    expect(api.runSlash).not.toHaveBeenCalled();
  });

  it("lists the chat commands as runnable, not greyed", async () => {
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "/");
    expect(await screen.findByRole("option", { name: /\/delete/ })).not.toHaveAttribute("aria-disabled");
    expect(screen.getByRole("option", { name: /\/rename/ })).not.toHaveAttribute("aria-disabled");
    expect(screen.getAllByText("In the cabin")).toHaveLength(1);
  });

  it("/delete asks first with the chat's name, and only Yes runs it", async () => {
    api.runSlash.mockResolvedValueOnce({
      kind: "confirm",
      prompt: "Delete Pier light? The chat and its files are removed for good.",
      yes: "/delete yes",
    });
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "/delete{Enter}");
    const card = await screen.findByRole("group", { name: "/delete asks first" });
    expect(card).toHaveTextContent("Delete Pier light?");
    expect(api.runSlash).toHaveBeenCalledTimes(1);
    api.runSlash.mockResolvedValueOnce({ kind: "deleted", id: "shell-x", title: "Pier light" });
    await userEvent.click(within(card).getByRole("button", { name: "Yes" }));
    await waitFor(() =>
      expect(api.runSlash).toHaveBeenLastCalledWith("/delete yes", expect.stringMatching(/^shell-/)),
    );
    expect(await screen.findByRole("status", { name: "/delete result" })).toHaveTextContent("Deleted Pier light");
    expect(screen.queryByRole("group", { name: "/delete asks first" })).toBeNull();
  });

  it("Cancel on a destructive question changes nothing and never sends the yes", async () => {
    api.runSlash.mockResolvedValueOnce({
      kind: "confirm",
      prompt: "Remove project Harbor (/work/harbor) from your list? Its files stay on disk.",
      yes: "/project delete yes",
    });
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "/project delete{Enter}");
    const card = await screen.findByRole("group", { name: "/project asks first" });
    await userEvent.click(within(card).getByRole("button", { name: "Cancel" }));
    expect(await screen.findByRole("status", { name: "/project result" })).toHaveTextContent(
      "Cancelled. Nothing changed.",
    );
    expect(api.runSlash).toHaveBeenCalledTimes(1);
    expect(api.runSlash).toHaveBeenCalledWith("/project delete", expect.stringMatching(/^shell-/));
  });

  it("/retry asks the last message again", async () => {
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "what is in this folder{Enter}");
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.getByLabelText("Message")).toBeEnabled());
    api.runSlash.mockResolvedValueOnce({ kind: "retry" });
    await userEvent.type(screen.getByLabelText("Message"), "/retry{Enter}");
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(2));
    expect(api.sendChat).toHaveBeenLastCalledWith(
      expect.stringMatching(/^shell-/),
      "what is in this folder",
      undefined,
    );
  });

  it("/retry with nothing sent yet says so, and /compact sends the engine's compact turn", async () => {
    api.runSlash.mockResolvedValueOnce({ kind: "retry" });
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "/retry{Enter}");
    expect(await screen.findByRole("status", { name: "/retry result" })).toHaveTextContent("Nothing to retry");
    expect(api.sendChat).not.toHaveBeenCalled();
    api.runSlash.mockResolvedValueOnce({ kind: "compact" });
    await userEvent.type(screen.getByLabelText("Message"), "/compact{Enter}");
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledWith(expect.stringMatching(/^shell-/), "/compact", "chat"));
  });

  it("/sessions opens History, and /undo and /stop with nothing running say so", async () => {
    const onOpenView = vi.fn();
    render(<Chat onOpenSettings={() => {}} onOpenView={onOpenView} />);
    api.runSlash.mockResolvedValueOnce({ kind: "open", view: "history" });
    await userEvent.type(screen.getByLabelText("Message"), "/sessions{Enter}");
    await waitFor(() => expect(onOpenView).toHaveBeenCalledWith("history"));
    api.runSlash.mockResolvedValueOnce({ kind: "undo" });
    await userEvent.type(screen.getByLabelText("Message"), "/undo{Enter}");
    expect(await screen.findByRole("status", { name: "/undo result" })).toHaveTextContent(UNDO_NA);
    api.runSlash.mockResolvedValueOnce({ kind: "stop" });
    await userEvent.type(screen.getByLabelText("Message"), "/stop{Enter}");
    expect(await screen.findByRole("status", { name: "/stop result" })).toHaveTextContent("Nothing is running");
  });
  it("lists the memory, skill and ledger commands as runnable, not greyed", async () => {
    const more: SlashHit[] = [
      { cmd: "/recall", hint: "Search chats and memory…", insert: "/recall ", runOnPick: false, disabled: null },
      { cmd: "/forget", hint: "Remove a memory topic (asks first)…", insert: "/forget ", runOnPick: false, disabled: null },
      { cmd: "/skills changes", hint: "What the app changed in your skills", insert: "/skills changes", runOnPick: true, disabled: null },
    ];
    api.slashHits.mockResolvedValue(more);
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "/");
    for (const name of [/\/recall/, /\/forget/, /\/skills changes/]) {
      expect(await screen.findByRole("option", { name })).not.toHaveAttribute("aria-disabled");
    }
  });

  it("/forget shows what goes, and only Yes sends the --yes line", async () => {
    api.runSlash.mockResolvedValueOnce({
      kind: "confirm",
      prompt: "Forget harbor? These go:\n- MEMORY.md: the harbor light is green",
      yes: "/forget --yes harbor",
    });
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "/forget harbor{Enter}");
    const card = await screen.findByRole("group", { name: "/forget asks first" });
    expect(card).toHaveTextContent("the harbor light is green");
    expect(api.runSlash).toHaveBeenCalledTimes(1);
    api.runSlash.mockResolvedValueOnce({ kind: "notice", text: "Forgot harbor: 1 note in the memory repo" });
    await userEvent.click(within(card).getByRole("button", { name: "Yes" }));
    await waitFor(() =>
      expect(api.runSlash).toHaveBeenLastCalledWith("/forget --yes harbor", expect.stringMatching(/^shell-/)),
    );
    expect(await screen.findByRole("status", { name: "/forget result" })).toBeInTheDocument();
  });

  it("Cancel on /memory prune retires nothing and never sends the yes", async () => {
    api.runSlash.mockResolvedValueOnce({
      kind: "confirm",
      prompt: "Retire 2 unsure notes? They stay on disk, marked forgotten.",
      yes: "/memory prune yes",
    });
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "/memory prune{Enter}");
    const card = await screen.findByRole("group", { name: "/memory asks first" });
    await userEvent.click(within(card).getByRole("button", { name: "Cancel" }));
    expect(await screen.findByRole("status", { name: "/memory result" })).toHaveTextContent(
      "Cancelled. Nothing changed.",
    );
    expect(api.runSlash).toHaveBeenCalledTimes(1);
  });

  it("/recall posts the matches and /skills opens Skills", async () => {
    const onOpenView = vi.fn();
    render(<Chat onOpenSettings={() => {}} onOpenView={onOpenView} />);
    api.runSlash.mockResolvedValueOnce({ kind: "notice", text: "Recalled for ferry\n- workspace: the ferry leaves at nine" });
    await userEvent.type(screen.getByLabelText("Message"), "/recall ferry{Enter}");
    expect(await screen.findByRole("status", { name: "/recall result" })).toHaveTextContent(
      "the ferry leaves at nine",
    );
    api.runSlash.mockResolvedValueOnce({ kind: "open", view: "skills" });
    await userEvent.type(screen.getByLabelText("Message"), "/skills{Enter}");
    await waitFor(() => expect(onOpenView).toHaveBeenCalledWith("skills"));
  });

  it("/hub where this app has no Devices page says so instead of doing nothing", async () => {
    const onOpenView = vi.fn(() => false);
    render(<Chat onOpenSettings={() => {}} onOpenView={onOpenView} />);
    api.runSlash.mockResolvedValueOnce({ kind: "open", view: "devices" });
    await userEvent.type(screen.getByLabelText("Message"), "/hub{Enter}");
    expect(await screen.findByRole("status", { name: "/hub result" })).toHaveTextContent(
      "/hub works on your computer.",
    );
    expect(onOpenView).toHaveBeenCalledWith("devices");
    // A page that opened leaves no line behind.
    onOpenView.mockReturnValue(true);
    api.runSlash.mockResolvedValueOnce({ kind: "open", view: "skills" });
    await userEvent.type(screen.getByLabelText("Message"), "/skills{Enter}");
    await waitFor(() => expect(onOpenView).toHaveBeenCalledWith("skills"));
    expect(screen.queryByRole("status", { name: "/skills result" })).toBeNull();
  });

  it("/queue with nothing running sends the message as a chat turn", async () => {
    api.runSlash.mockResolvedValueOnce({ kind: "queue", text: "then run the tests" });
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "/queue then run the tests{Enter}");
    await waitFor(() =>
      expect(api.sendChat).toHaveBeenCalledWith(expect.stringMatching(/^shell-/), "then run the tests", "chat"),
    );
  });

  it("/board opens Workboards, /palette opens the palette, and /view-plan with no plan says so", async () => {
    const onOpenView = vi.fn();
    const onOpenPalette = vi.fn();
    render(<Chat onOpenSettings={() => {}} onOpenView={onOpenView} onOpenPalette={onOpenPalette} />);
    api.runSlash.mockResolvedValueOnce({ kind: "open", view: "workboard" });
    await userEvent.type(screen.getByLabelText("Message"), "/board{Enter}");
    await waitFor(() => expect(onOpenView).toHaveBeenCalledWith("workboard"));
    api.runSlash.mockResolvedValueOnce({ kind: "palette" });
    await userEvent.type(screen.getByLabelText("Message"), "/palette{Enter}");
    await waitFor(() => expect(onOpenPalette).toHaveBeenCalledTimes(1));
    api.runSlash.mockResolvedValueOnce({ kind: "viewPlan" });
    await userEvent.type(screen.getByLabelText("Message"), "/view-plan{Enter}");
    expect(await screen.findByRole("status", { name: "/view-plan result" })).toHaveTextContent(NO_PLAN);
    expect(NO_PLAN).toBe("No plan yet. Use Plan mode.");
  });

  it("/loop saves its words as an automation and opens Automations; words with no time say why", async () => {
    const onOpenView = vi.fn();
    render(<Chat onOpenSettings={() => {}} onOpenView={onOpenView} />);
    api.addAutomation.mockResolvedValueOnce([]);
    api.runSlash.mockResolvedValueOnce({ kind: "loop", seed: "every day at 9 summarize my inbox" });
    await userEvent.type(screen.getByLabelText("Message"), "/loop every day at 9 summarize my inbox{Enter}");
    await waitFor(() => expect(api.addAutomation).toHaveBeenCalledWith("every day at 9 summarize my inbox"));
    await waitFor(() => expect(onOpenView).toHaveBeenCalledWith("automations"));
    expect(await screen.findByRole("status", { name: "/loop result" })).toHaveTextContent(
      "Scheduled: every day at 9 summarize my inbox",
    );
    api.addAutomation.mockRejectedValueOnce("Say when, like \"every weekday at 9am summarize my inbox\".");
    api.runSlash.mockResolvedValueOnce({ kind: "loop", seed: "summarize my inbox" });
    await userEvent.type(screen.getByLabelText("Message"), "/loop summarize my inbox{Enter}");
    await waitFor(() => expect(screen.getAllByRole("status", { name: "/loop result" })).toHaveLength(2));
    expect(onOpenView).toHaveBeenCalledTimes(1);
  });

  it("/auto opens Settings with the line saying where approvals live", async () => {
    const onOpenView = vi.fn();
    render(<Chat onOpenSettings={() => {}} onOpenView={onOpenView} />);
    api.runSlash.mockResolvedValueOnce({
      kind: "openNote",
      view: "settings",
      text: "Permissions are set in Settings → Defaults, under When the AI wants to change things",
    });
    await userEvent.type(screen.getByLabelText("Message"), "/auto{Enter}");
    await waitFor(() => expect(onOpenView).toHaveBeenCalledWith("settings"));
    expect(await screen.findByRole("status", { name: "/auto result" })).toHaveTextContent("Defaults");
  });

  describe("while the list for the newest keystroke is still on its way", () => {
    const CLEAR: SlashHit = { cmd: "/clear", hint: "New chat", insert: "/clear", runOnPick: true, disabled: null };
    const CHAT: SlashHit = { cmd: "/chat", hint: "Chat mode", insert: "/chat", runOnPick: true, disabled: null };
    /** `/c` answers at once; every later draft waits for `answer`. */
    function slowAfterC() {
      let answer: (rows: SlashHit[]) => void = () => {};
      api.slashHits.mockImplementation((draft) =>
        draft === "/c" ? Promise.resolve([CLEAR, CHAT]) : new Promise((resolve) => (answer = resolve)),
      );
      return (rows: SlashHit[]) => answer(rows);
    }

    it("Enter runs the line as typed, not the top row of the last keystroke's list", async () => {
      slowAfterC();
      api.runSlash.mockRejectedValue("Unknown command /ch. Type /help for the list.");
      render(<Chat onOpenSettings={() => {}} />);
      const box = screen.getByLabelText("Message");
      await userEvent.type(box, "/c");
      await screen.findByRole("option", { name: /\/clear/ });
      await userEvent.type(box, "h{Enter}");
      await waitFor(() => expect(api.runSlash).toHaveBeenCalled());
      expect(api.runSlash.mock.calls.map(([line]) => line)).toEqual(["/ch"]);
      expect(await screen.findByRole("alert")).toHaveTextContent("Unknown command /ch");
      expect(api.closeChat).not.toHaveBeenCalled();
    });

    it("Tab waits for the list, then fills in its pick", async () => {
      const answer = slowAfterC();
      render(<Chat onOpenSettings={() => {}} />);
      const box = screen.getByLabelText("Message");
      await userEvent.type(box, "/c");
      await screen.findByRole("option", { name: /\/clear/ });
      await userEvent.type(box, "h{Tab}");
      expect(box).toHaveValue("/ch");
      expect(box).toHaveFocus();
      answer([CHAT]);
      await waitFor(() => expect(screen.queryByRole("option", { name: /\/clear/ })).toBeNull());
      await userEvent.keyboard("{Tab}");
      expect(box).toHaveValue("/chat");
      expect(api.runSlash).not.toHaveBeenCalled();
    });
  });
});

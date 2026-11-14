import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { DELETE_ASK_QUESTION, DELETE_MESSAGE_LABEL, DELETE_REPLY_QUESTION, MESSAGE_MENU_LABEL } from "./DeleteMessage";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  Answer,
  Approvals,
  ChatEnvelope,
  ChatEvent,
  ElicitAction,
  QuickChip,
  SessionMode,
  Settings,
  Transcript,
} from "./api";

const api = vi.hoisted(() => ({
  handler: null as ((envelope: ChatEnvelope) => void) | null,
  getDeviceGlance: vi.fn<() => Promise<{ sharing: boolean; frame: string | null } | null>>(() =>
    Promise.resolve(null),
  ),
  gap: null as (() => void) | null,
  sendChat: vi.fn<(id: string, text: string, mode?: SessionMode) => Promise<string>>(),
  answerElicit:
    vi.fn<
      (id: string, callId: string, action: ElicitAction, field?: string | null, value?: string) => Promise<void>
    >(),
  getSettings: vi.fn<() => Promise<Settings>>(),
  getBehavior: vi.fn(() =>
    Promise.resolve({
      closeToTray: true,
      behavior: {
        autocomplete: false,
        suggestions: true,
        quietStart: "",
        quietEnd: "",
        dailyTokenBudget: 0,
        pauseOverBudget: false,
        lane: "coding",
      },
      budgets: [],
    }),
  ),
  composerGhostSuggest: vi.fn<(draft: string) => Promise<string>>(() => Promise.resolve("")),
  saveSettings:
    vi.fn<
      (tray: boolean, ws: string, approvals: Approvals, desk: boolean) => Promise<Settings>
    >(),
  answerPermission: vi.fn<(id: string, callId: string, answer: Answer) => Promise<void>>(),
  cancelChat: vi.fn<(id: string) => Promise<void>>(),
  steerChat: vi.fn<(id: string, text: string) => Promise<boolean>>(),
  readTurn: vi.fn((text: string) => Promise.resolve({ route: "queue", text, ack: "Queued for after this reply" })),
  sideChat:
    vi.fn<(id: string, sideId: string, question: string, ask: string, soFar: string) => Promise<string>>(),
  tellMain: vi.fn<(id: string, question: string, answer: string) => Promise<{ text: string; steered: boolean }>>(),
  closeChat: vi.fn<(id: string) => Promise<void>>(),
  clearChat: vi.fn<(id: string) => Promise<Transcript | null>>(),
  forgetMessage:
    vi.fn<(id: string, cut: { reply: boolean; text: string | null; later: number }) => Promise<boolean>>(),
  getStartChat: vi.fn<() => Promise<Transcript | null>>(),
  loadChat: vi.fn<(id: string, limit?: number) => Promise<Transcript>>(),
  slashHits: vi.fn(() => Promise.resolve([])),
  runSlash: vi.fn(),
  exportChat: vi.fn(),
  openSaved: vi.fn(),
  revealSaved: vi.fn(),
  quickChips: vi.fn<(surface: string, id?: string, draft?: string, mode?: SessionMode) => Promise<QuickChip[]>>(),
  chipOutcome: vi.fn<(surface: string, id: string, picked: boolean) => Promise<boolean>>(),
  codeSessionPlan: vi.fn<(id: string) => Promise<string | null>>(),
  decisionInbox: vi.fn(() => Promise.resolve([])),
  answerInbox: vi.fn(() => Promise.resolve("answered")),
  replyPick: vi.fn(() => Promise.resolve()),
  listChats: vi.fn(() => Promise.resolve([])),
  goalAct: vi.fn(() => Promise.resolve({ board: { today: [], mine: [], suggested: [], season: { label: "", done: 0, trophies: [], badges: [] } } })),
  getHangoutMode: vi.fn(() => Promise.resolve("type" as const)),
  setHangoutMode: vi.fn((mode: "talk" | "type") => Promise.resolve(mode)),
  listChatTabs: vi.fn(() => Promise.resolve([])),
  listArchivedChats: vi.fn(() => Promise.resolve([])),
  listDeletedChats: vi.fn(() => Promise.resolve([])),
  chatTabSwitch: vi.fn(),
  chatTabPin: vi.fn(),
  chatTabRename: vi.fn(),
  chatTabArchive: vi.fn(),
  chatTabUnarchive: vi.fn(),
  chatTabSoftDelete: vi.fn(),
  chatTabRestoreDeleted: vi.fn(),
}));

// The router's no-route wait: the test says when a model is back.
const wait = vi.hoisted(() => ({
  back: null as (() => void) | null,
  modelResume: vi.fn<(id: string) => Promise<{ conversationId: string; ask: string; note: string } | null>>(
    () => Promise.resolve(null),
  ),
}));
vi.mock("./modelWait", () => ({
  modelResume: wait.modelResume,
  onModelBack: (handler: () => void) => {
    wait.back = handler;
    return Promise.resolve(() => {});
  },
}));

// A chat line that asks for a setting is answered by auto setup.
const auto = vi.hoisted(() => ({
  settingAsk: vi.fn<(text: string) => Promise<string | null>>(() => Promise.resolve(null)),
}));
vi.mock("./autoSetupApi", () => auto);

vi.mock("./api", () => ({
  getDeviceGlance: api.getDeviceGlance,
  sendChat: api.sendChat,
  answerElicit: api.answerElicit,
  getSettings: api.getSettings,
  getBehavior: api.getBehavior,
  composerGhostSuggest: api.composerGhostSuggest,
  saveSettings: api.saveSettings,
  answerPermission: api.answerPermission,
  cancelChat: api.cancelChat,
  steerChat: api.steerChat,
  readTurn: api.readTurn,
  sideChat: api.sideChat,
  tellMain: api.tellMain,
  closeChat: api.closeChat,
  clearChat: api.clearChat,
  forgetMessage: api.forgetMessage,
  getStartChat: api.getStartChat,
  loadChat: api.loadChat,
  slashHits: api.slashHits,
  runSlash: api.runSlash,
  exportChat: api.exportChat,
  openSaved: api.openSaved,
  revealSaved: api.revealSaved,
  quickChips: api.quickChips,
  chipOutcome: api.chipOutcome,
  decisionInbox: api.decisionInbox,
  answerInbox: api.answerInbox,
  replyPick: api.replyPick,
  listChats: api.listChats,
  goalAct: api.goalAct,
  codeSessionPlan: api.codeSessionPlan,
  getHangoutMode: api.getHangoutMode,
  setHangoutMode: api.setHangoutMode,
  listChatTabs: api.listChatTabs,
  listArchivedChats: api.listArchivedChats,
  listDeletedChats: api.listDeletedChats,
  chatTabSwitch: api.chatTabSwitch,
  chatTabPin: api.chatTabPin,
  chatTabRename: api.chatTabRename,
  chatTabArchive: api.chatTabArchive,
  chatTabUnarchive: api.chatTabUnarchive,
  chatTabSoftDelete: api.chatTabSoftDelete,
  chatTabRestoreDeleted: api.chatTabRestoreDeleted,
  onChatEvent: (handler: (envelope: ChatEnvelope) => void) => {
    api.handler = handler;
    return Promise.resolve(() => {});
  },
  helperQuestions: () => Promise.resolve([]),
  onEventGap: (handler: () => void) => {
    api.gap = handler;
    return Promise.resolve(() => {});
  },
  // Push to talk has its own tests (Mic.test.tsx).
  transcribe: vi.fn(),
  // Attachments have their own tests (AttachChips.test.tsx).
  onFileDrop: () => Promise.resolve(() => {}),
  stageAttachment: vi.fn(),
  stageAttachmentPaths: vi.fn(),
  pickAttachments: vi.fn(),
  removeAttachment: vi.fn(),
}));

import {
  AGENT_EMPTY,
  AGENT_PLACEHOLDER,
  applyEvent,
  Chat,
  CHAT_EMPTY,
  EARLIER_STEP,
  EVENTS_LOST,
  HALTED_LINE,
  NO_PLAN,
  isScratchChat,
  itemsFrom,
  MISSION_BUSY,
  newConversationId,
  OPEN_BUSY,
  PLAN_NEXT,
  STOP_ASKING_DONE,
  type Item,
  waitingLine,
  type QueueAct,
} from "./Chat";
import { ASK_TRACKED } from "./Goals";

const SAVED: Transcript = {
  id: "shell-saved",
  title: "paint colours",
  lines: [
    { role: "user", text: "pick a paint colour" },
    { role: "tools", text: "read_file, web_search" },
    { role: "assistant", text: "Try **blue**." },
  ],
  earlier: 0,
};

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

function chip(id: string, label: string, value: string, over: Partial<QuickChip> = {}): QuickChip {
  return {
    id,
    label,
    value,
    action: "ask",
    hint: id.startsWith("reply-") ? "Suggested in the reply" : "",
    primary: false,
    ...over,
  };
}

/** Holds sendChat open until `finish` is called, like a turn in flight. */
function holdTurn() {
  let finish: (stop?: string) => void = () => {};
  api.sendChat.mockImplementation(
    () => new Promise<string>((resolve) => (finish = (stop = "end_turn") => resolve(stop))),
  );
  return () => finish();
}

async function startTurn(text: string) {
  await userEvent.type(screen.getByLabelText("Message"), `${text}{Enter}`);
  await waitFor(() => expect(api.sendChat).toHaveBeenCalled());
  return api.sendChat.mock.calls[api.sendChat.mock.calls.length - 1][0];
}

function emit(conversationId: string, event: ChatEvent) {
  act(() => api.handler?.({ conversationId, event }));
}

/** A question card; with `labels` it offers them as buttons. */
function question(id: string, message: string, labels: string[]): ChatEvent {
  return {
    kind: "elicit",
    id,
    server: "Question",
    message,
    mode: "form",
    url: "",
    fieldName: "answer",
    fieldTitle: "Answer",
    secret: false,
    options: labels.map((label) => ({ label, description: "", recommended: false })),
  };
}

describe("Chat", () => {
  beforeEach(() => {
    api.getDeviceGlance.mockReset().mockResolvedValue(null);
    for (const fn of [
      api.sendChat,
      api.answerPermission,
      api.cancelChat,
      api.closeChat,
      api.answerElicit,
      api.saveSettings,
      api.loadChat,
    ]) {
      fn.mockReset();
    }
    // The real call always answers with a promise; the page handles its refusal.
    api.closeChat.mockResolvedValue();
    api.getStartChat.mockReset().mockResolvedValue(null);
    api.quickChips.mockReset().mockResolvedValue([]);
    api.chipOutcome.mockReset().mockResolvedValue(true);
    api.getSettings.mockReset().mockResolvedValue(SETTINGS);
    api.getBehavior.mockReset().mockResolvedValue({
      closeToTray: true,
      behavior: {
        autocomplete: false,
        suggestions: true,
        quietStart: "",
        quietEnd: "",
        dailyTokenBudget: 0,
        pauseOverBudget: false,
        lane: "coding",
      },
      budgets: [],
    });
    api.composerGhostSuggest.mockReset().mockResolvedValue("");
    api.handler = null;
    api.gap = null;
  });

  it("Tab accepts a ghost suggestion when slash is closed; typing clears it", async () => {
    api.getBehavior.mockResolvedValue({
      closeToTray: true,
      behavior: {
        autocomplete: true,
        suggestions: true,
        quietStart: "",
        quietEnd: "",
        dailyTokenBudget: 0,
        pauseOverBudget: false,
        lane: "coding",
      },
      budgets: [],
    });
    api.composerGhostSuggest.mockResolvedValue(" world");
    render(<Chat onOpenSettings={() => {}} />);
    const box = await screen.findByRole("textbox", { name: "Message" });
    await userEvent.type(box, "hello");
    await waitFor(() => expect(api.composerGhostSuggest).toHaveBeenCalledWith("hello"), {
      timeout: 1000,
    });
    await waitFor(() => expect(document.querySelector(".composer-ghost-sug")?.textContent).toBe(" world"));
    fireEvent.keyDown(box, { key: "Tab" });
    expect(box).toHaveValue("hello world");
    expect(document.querySelector(".composer-ghost-sug")).toBeNull();

    api.composerGhostSuggest.mockClear().mockResolvedValue(" again");
    await userEvent.type(box, "!");
    expect(document.querySelector(".composer-ghost-sug")).toBeNull();
  });

  it("says part of the reply may be missing after a gap in the web app's events", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    act(() => api.gap?.());
    expect(screen.queryByText(EVENTS_LOST)).toBeNull();
    await startTurn("hello");
    act(() => api.gap?.());
    expect(screen.getByText(EVENTS_LOST)).toBeVisible();
    finish();
  });

  it("a turn no model answered goes again once by itself when one is back, with the note", async () => {
    const paused = "protocol error: No model in your plan is answering right now.";
    api.sendChat.mockRejectedValueOnce(paused).mockResolvedValueOnce("end_turn");
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("Summarize inbox");
    expect(await screen.findByText(paused)).toBeVisible();

    wait.modelResume.mockImplementation((asked) =>
      Promise.resolve(
        asked === id
          ? { conversationId: id, ask: "Summarize inbox", note: "xAI is answering again; resumed: Summarize inbox" }
          : null,
      ),
    );
    act(() => wait.back?.());
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(2));
    expect(api.sendChat.mock.calls[1].slice(0, 2)).toEqual([id, "Summarize inbox"]);
    expect(await screen.findByText("xAI is answering again; resumed: Summarize inbox")).toBeVisible();
    // The ask is on screen once: going again adds no second line.
    expect(screen.getAllByText("Summarize inbox")).toHaveLength(1);
    expect(screen.queryByText(paused)).toBeNull();
    wait.modelResume.mockReset().mockResolvedValue(null);
  });

  it("puts a prefill in the box without sending it, and a new one replaces it", async () => {
    const { rerender } = render(
      <Chat onOpenSettings={() => {}} prefill={{ text: "close the done cards", n: 1 }} />,
    );
    const box = screen.getByRole("textbox", { name: "Message" });
    expect(box).toHaveValue("close the done cards");
    expect(api.sendChat).not.toHaveBeenCalled();

    rerender(<Chat onOpenSettings={() => {}} prefill={{ text: "check the build", n: 2 }} />);
    expect(box).toHaveValue("check the build");
    expect(api.sendChat).not.toHaveBeenCalled();
  });

  it("TALK and TYPE keep the draft in the box", async () => {
    api.getHangoutMode.mockResolvedValue("type");
    api.setHangoutMode.mockImplementation(async (mode) => mode);
    render(<Chat onOpenSettings={() => {}} />);
    const box = screen.getByRole("textbox", { name: "Message" });
    await userEvent.type(box, "keep me while switching");
    expect(await screen.findByRole("group", { name: "Hangout mode" })).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "TALK" }));
    await waitFor(() => expect(api.setHangoutMode).toHaveBeenCalledWith("talk"));
    expect(box).toHaveValue("keep me while switching");
    await userEvent.click(screen.getByRole("button", { name: "TYPE" }));
    await waitFor(() => expect(api.setHangoutMode).toHaveBeenCalledWith("type"));
    expect(box).toHaveValue("keep me while switching");
  });

  it("a send prefill (the Command line's run) goes as a chat message, leaving the box alone", async () => {
    api.sendChat.mockResolvedValue("end_turn");
    const { rerender } = render(<Chat onOpenSettings={() => {}} />);
    const box = screen.getByRole("textbox", { name: "Message" });
    await userEvent.type(box, "half typed");
    rerender(<Chat onOpenSettings={() => {}} prefill={{ text: "Run this: ls", n: 1, send: true }} />);
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(1));
    expect(api.sendChat.mock.calls[0].slice(1)).toEqual(["Run this: ls", "chat"]);
    expect(box).toHaveValue("half typed");
  });

  it("a send prefill behind a running reply waits its turn", async () => {
    const finish = holdTurn();
    const { rerender } = render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("hello");
    rerender(<Chat onOpenSettings={() => {}} prefill={{ text: "Run this: ls", n: 1, send: true }} />);
    expect(api.sendChat).toHaveBeenCalledTimes(1);
    emit(id, { kind: "done", stop: "end_turn" });
    finish();
    await waitFor(() => expect(api.sendChat).toHaveBeenLastCalledWith(id, "Run this: ls", undefined));
  });

  it("a findings pill fills the composer without sending", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("scan it");
    emit(id, { kind: "text", text: "Scan complete." });
    emit(id, {
      kind: "findings",
      items: [{ severity: "high", text: "Port 53 is taken" }],
      fixes: [{ label: "Fix port 53 conflict", prompt: "Fix port 53 conflict: Stop the other service" }],
      doAll: null,
    });
    emit(id, { kind: "done", stop: "end_turn" });
    finish();
    await userEvent.click(screen.getByRole("button", { name: "Fix port 53 conflict" }));
    expect(screen.getByRole("textbox", { name: "Message" })).toHaveValue(
      "Fix port 53 conflict: Stop the other service",
    );
    expect(api.sendChat).toHaveBeenCalledTimes(1);
  });

  const PANEL = {
    title: "Linux CI",
    elements: [
      { type: "heading" as const, level: 2, text: "Checks" },
      { type: "key_value" as const, pairs: [{ key: "Workflow", value: "ci" }, { key: "Branch", value: "beta" }] },
      { type: "button" as const, label: "Re-run", action: "rerun_ci" },
    ],
  };

  it("a panel button sends its action as the next turn, and waits behind a running reply", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("show CI");
    emit(id, { kind: "panel", id: "call-1", spec: PANEL });
    const card = await screen.findByRole("group", { name: "Linux CI" });
    expect(card).toHaveTextContent("Workflow:ci");
    expect(card).toHaveTextContent("Branch:beta");
    // While the reply runs, the tap lines up behind it.
    await userEvent.click(screen.getByRole("button", { name: "Re-run" }));
    expect(api.sendChat).toHaveBeenCalledTimes(1);
    emit(id, { kind: "done", stop: "end_turn" });
    finish();
    await waitFor(() =>
      expect(api.sendChat).toHaveBeenLastCalledWith(id, "[panel action] rerun_ci on call-1", undefined),
    );
  });

  it("a panel tapped when idle sends at once; a new spec for its id replaces it, and Dismiss keeps the same one away", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("show CI");
    emit(id, { kind: "panel", id: "call-1", spec: PANEL });
    emit(id, { kind: "panel", id: "call-1", spec: { ...PANEL, title: "Linux CI (rerun)" } });
    expect(await screen.findByRole("group", { name: "Linux CI (rerun)" })).toBeInTheDocument();
    expect(screen.queryByRole("group", { name: "Linux CI" })).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByRole("group", { name: "Linux CI (rerun)" })).toBeNull();
    emit(id, { kind: "panel", id: "call-1", spec: { ...PANEL, title: "Linux CI (rerun)" } });
    await act(() => Promise.resolve());
    expect(screen.queryByRole("group", { name: "Linux CI (rerun)" })).toBeNull();
    emit(id, { kind: "panel", id: "call-1", spec: { ...PANEL, title: "Linux CI (green)" } });
    expect(await screen.findByRole("group", { name: "Linux CI (green)" })).toBeInTheDocument();
    emit(id, { kind: "done", stop: "end_turn" });
    finish();
    await waitFor(() => expect(screen.getByRole("button", { name: "Re-run" })).toBeEnabled());
    await userEvent.click(screen.getByRole("button", { name: "Re-run" }));
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(2));
    expect(api.sendChat.mock.calls[1].slice(1)).toEqual(["[panel action] rerun_ci on call-1", undefined]);
    expect(await screen.findByText("[panel action] rerun_ci on call-1")).toBeInTheDocument();
  });

  it("panels with one repeated id keep a card per turn, and Dismiss hides one without scrolling", async () => {
    // A provider that sends no call ids: every panel is "tool-show_panel".
    let finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("show CI");
    emit(id, { kind: "panel", id: "tool-show_panel", spec: { ...PANEL, title: "Monday CI" } });
    emit(id, { kind: "done", stop: "end_turn" });
    finish();
    await waitFor(() => expect(screen.getByLabelText("Message")).toBeEnabled());
    finish = holdTurn();
    await startTurn("and now?");
    emit(id, { kind: "panel", id: "tool-show_panel", spec: { ...PANEL, title: "Tuesday CI" } });
    emit(id, { kind: "panel", id: "tool-show_panel", spec: { ...PANEL, title: "Tuesday CI (green)" } });
    emit(id, { kind: "done", stop: "end_turn" });
    finish();
    expect(await screen.findByRole("group", { name: "Tuesday CI (green)" })).toBeInTheDocument();
    expect(screen.getByRole("group", { name: "Monday CI" })).toBeInTheDocument();
    expect(screen.queryByRole("group", { name: "Tuesday CI" })).toBeNull();
    // jsdom has no scrollIntoView; the log calls it when it follows the end.
    const scroll = vi.fn();
    Element.prototype.scrollIntoView = scroll;
    const monday = screen.getByRole("group", { name: "Monday CI" });
    await userEvent.click(within(monday).getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByRole("group", { name: "Monday CI" })).toBeNull();
    expect(screen.getByRole("group", { name: "Tuesday CI (green)" })).toBeInTheDocument();
    expect(scroll).not.toHaveBeenCalled();
    // @ts-expect-error -- put jsdom back as it was
    delete Element.prototype.scrollIntoView;
  });

  it("Ctrl+F finds in the chat: newest match first, Enter and Shift+Enter step and wrap, Esc closes", async () => {
    api.getStartChat.mockResolvedValue({
      id: "shell-find",
      title: "build",
      lines: [
        { role: "user", text: "build the cabin" },
        { role: "assistant", text: "Building it." },
        { role: "user", text: "now test" },
        { role: "assistant", text: "Tests pass. Build is green." },
      ],
      earlier: 0,
    });
    render(<Chat onOpenSettings={() => {}} />);
    await screen.findByText("now test");
    await userEvent.keyboard("{Control>}f{/Control}");
    const box = screen.getByRole("textbox", { name: "Find in this chat" });
    expect(box).toHaveFocus();
    await userEvent.type(box, "BUILD");
    const bar = screen.getByRole("search", { name: "Find in chat" });
    expect(bar).toHaveTextContent("3 of 3");
    const row = (text: string) => screen.getByText(text).closest("[data-role]");
    expect(row("Tests pass. Build is green.")).toHaveAttribute("data-find", "current");
    expect(row("build the cabin")).toHaveAttribute("data-find", "hit");
    expect(row("now test")).not.toHaveAttribute("data-find");
    await userEvent.keyboard("{Enter}");
    expect(bar).toHaveTextContent("1 of 3");
    expect(row("build the cabin")).toHaveAttribute("data-find", "current");
    expect(row("Tests pass. Build is green.")).toHaveAttribute("data-find", "hit");
    await userEvent.keyboard("{Shift>}{Enter}{/Shift}");
    expect(bar).toHaveTextContent("3 of 3");
    await userEvent.clear(box);
    await userEvent.type(box, "zzz");
    expect(bar).toHaveTextContent("No matches");
    expect(screen.getByRole("button", { name: "Next match" })).toBeDisabled();
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("search", { name: "Find in chat" })).toBeNull();
    expect(screen.getByLabelText("Message")).toHaveFocus();
    expect(document.querySelector("[data-find]")).toBeNull();
  });

  it("while find is open a streaming reply leaves the view and the pick on the match", async () => {
    api.getStartChat.mockResolvedValue(SAVED);
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    await screen.findByText("pick a paint colour");
    const id = await startTurn("and the trim?");
    await userEvent.keyboard("{Control>}f{/Control}");
    // jsdom draws nothing, so it has no scrollIntoView; the log calls it when there is one.
    const scroll = vi.fn();
    Element.prototype.scrollIntoView = scroll;
    try {
      await userEvent.type(screen.getByRole("textbox", { name: "Find in this chat" }), "paint");
      expect(scroll.mock.calls).toContainEqual([{ block: "center" }]);
      expect(scroll.mock.calls).not.toContainEqual([{ block: "end" }]);
      scroll.mockClear();
      emit(id, { kind: "text", text: "Paint the trim white." });
      expect(screen.getByText("Paint the trim white.")).toBeVisible();
      expect(scroll).not.toHaveBeenCalled();
      // The new match joins the count; the person stays on the one they were reading.
      expect(screen.getByRole("search", { name: "Find in chat" })).toHaveTextContent("1 of 2");
      expect(screen.getByText("pick a paint colour").closest("[data-role]")).toHaveAttribute("data-find", "current");
      await userEvent.keyboard("{Escape}");
      expect(scroll).not.toHaveBeenCalled();
      emit(id, { kind: "text", text: " And the door." });
      expect(scroll).toHaveBeenCalledWith({ block: "end" });
    } finally {
      delete (Element.prototype as Partial<Element>).scrollIntoView;
    }
    emit(id, { kind: "done", stop: "end_turn" });
    finish();
  });

  it("Ctrl+F stays out of an open dialog, a held key and another letter on the F key", async () => {
    api.getStartChat.mockResolvedValue(SAVED);
    render(
      <>
        <Chat onOpenSettings={() => {}} />
        <div role="dialog" aria-modal="true" aria-label="Palette">
          <input aria-label="Search pages and commands" />
        </div>
      </>,
    );
    await screen.findByText("pick a paint colour");
    const palette = screen.getByRole("textbox", { name: "Search pages and commands" });
    palette.focus();
    await userEvent.keyboard("{Control>}f{/Control}");
    expect(screen.queryByRole("search", { name: "Find in chat" })).toBeNull();
    expect(palette).toHaveFocus();
  });

  it("Ctrl+F follows the layout's letter, falls back to the key's place, and ignores a held key", async () => {
    api.getStartChat.mockResolvedValue(SAVED);
    render(<Chat onOpenSettings={() => {}} />);
    await screen.findByText("pick a paint colour");
    const bar = () => screen.queryByRole("search", { name: "Find in chat" });
    // Dvorak's U sits where QWERTY has F.
    fireEvent.keyDown(window, { key: "u", code: "KeyF", ctrlKey: true });
    expect(bar()).toBeNull();
    fireEvent.keyDown(window, { key: "f", code: "KeyF", ctrlKey: true, repeat: true });
    expect(bar()).toBeNull();
    // A Cyrillic layout types а on that key: no Latin letter, so its place counts.
    fireEvent.keyDown(window, { key: "а", code: "KeyF", ctrlKey: true });
    expect(bar()).not.toBeNull();
    fireEvent.keyDown(window, { key: "f", code: "KeyF", ctrlKey: true, repeat: true });
    expect(bar()).not.toBeNull();
  });

  it("Ctrl+F does nothing on an empty chat or while the Chat tab is hidden", async () => {
    const view = render(<Chat onOpenSettings={() => {}} />);
    await userEvent.keyboard("{Control>}f{/Control}");
    expect(screen.queryByRole("search", { name: "Find in chat" })).toBeNull();
    api.getStartChat.mockResolvedValue(SAVED);
    view.unmount();
    render(<Chat onOpenSettings={() => {}} active={false} />);
    await screen.findByText("pick a paint colour");
    await userEvent.keyboard("{Control>}f{/Control}");
    expect(screen.queryByRole("search", { name: "Find in chat" })).toBeNull();
  });

  it("a recap card swaps the Finished block out of the bubble", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("do the work");
    emit(id, { kind: "text", text: "Fixed it.\n\nFinished\n- Done: Read the file" });
    emit(id, {
      kind: "recap",
      body: "Fixed it.",
      lines: [{ status: "done", text: "Read the file" }],
      needsYou: [],
      undo: [],
      steps: [{ title: "read_file", failed: false }],
    });
    emit(id, { kind: "done", stop: "end_turn" });
    finish();
    expect(screen.getByText("1 of 1 done")).toBeVisible();
    const bubble = document.querySelector('[data-role="assistant"]');
    expect(bubble?.textContent).toBe("Fixed it.");
    await userEvent.click(screen.getByRole("button", { name: "Show details" }));
    expect(screen.getByText("read_file")).toBeVisible();
  });

  it("thinking streams into one thought above the reply, and a tool between starts another", () => {
    let items: Item[] = [{ kind: "user", text: "snap it" }];
    for (const event of [
      { kind: "thought", text: "Need a " },
      { kind: "thought", text: "snapshot." },
      { kind: "tool", id: "t1", title: "read_file", status: "done", detail: "" },
      { kind: "thought", text: "Now write." },
      { kind: "text", text: "Done." },
    ] satisfies ChatEvent[]) {
      items = applyEvent(items, event);
    }
    expect(items).toEqual([
      { kind: "user", text: "snap it" },
      { kind: "thought", text: "Need a snapshot." },
      { kind: "tool", id: "t1", title: "read_file", status: "done", detail: "" },
      { kind: "thought", text: "Now write." },
      { kind: "assistant", text: "Done.", streaming: true },
    ]);
  });

  it("thinking that lands mid-reply goes above it, so the reply stays one bubble for its recap", () => {
    let items: Item[] = [{ kind: "user", text: "write it" }];
    for (const event of [
      { kind: "tool", id: "t1", title: "read_file", status: "done", detail: "" },
      { kind: "text", text: "Let me write it." },
      { kind: "thought", text: "Waiting for the " },
      { kind: "thought", text: "model to come back." },
      { kind: "text", text: " Done.\n\nFinished\n- Done: wrote" },
      { kind: "recap", body: "Let me write it. Done.", lines: [], needsYou: [], undo: [], steps: [] },
    ] satisfies ChatEvent[]) {
      items = applyEvent(items, event);
    }
    expect(items.filter((item) => item.kind === "assistant")).toEqual([
      { kind: "assistant", text: "Let me write it. Done.", streaming: true },
    ]);
    expect(items.map((item) => item.kind)).toEqual(["user", "tool", "thought", "assistant", "recap"]);
    expect(items[2]).toEqual({ kind: "thought", text: "Waiting for the model to come back." });
  });

  it("Collapse and Expand leave the log where it is", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("snap it");
    emit(id, { kind: "thought", text: "Need a snapshot." });
    emit(id, { kind: "text", text: "Done." });
    emit(id, { kind: "done", stop: "end_turn" });
    finish();
    // The turn has settled: the reply stopped streaming, so nothing new is coming.
    await waitFor(() => expect(document.querySelector(".bubble.assistant.streaming")).toBeNull());
    await waitFor(() => expect(screen.getByRole("button", { name: "New chat" })).toBeEnabled());
    // jsdom draws nothing, so it has no scrollIntoView; the log calls it when there is one.
    const scroll = vi.fn();
    Element.prototype.scrollIntoView = scroll;
    try {
      await userEvent.click(screen.getByRole("button", { name: "Collapse" }));
      await userEvent.click(screen.getByRole("button", { name: "Expand" }));
      expect(screen.getByText("Need a snapshot.")).toBeVisible();
      expect(scroll).not.toHaveBeenCalled();
    } finally {
      delete (Element.prototype as Partial<Element>).scrollIntoView;
    }
  });

  it("find searches thoughts, and folding them keeps the match the person is on", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("snap it");
    emit(id, { kind: "thought", text: "Need a snapshot." });
    emit(id, { kind: "tool", id: "t1", title: "read_file", status: "done", detail: "" });
    emit(id, { kind: "thought", text: "Write the snapshot." });
    emit(id, { kind: "text", text: "Snapshot saved." });
    emit(id, { kind: "done", stop: "end_turn" });
    finish();
    await waitFor(() => expect(document.querySelector(".bubble.assistant.streaming")).toBeNull());
    await userEvent.keyboard("{Control>}f{/Control}");
    await userEvent.type(screen.getByRole("textbox", { name: "Find in this chat" }), "snapshot");
    const bar = screen.getByRole("search", { name: "Find in chat" });
    expect(bar).toHaveTextContent("3 of 3");
    await userEvent.keyboard("{Shift>}{Enter}{/Shift}");
    expect(bar).toHaveTextContent("2 of 3");
    expect(document.querySelector('[data-find="current"]')?.textContent).toContain("Write the snapshot.");
    await userEvent.click(screen.getAllByRole("button", { name: "Collapse" })[1]);
    expect(screen.queryByText("Write the snapshot.")).toBeNull();
    expect(bar).toHaveTextContent("2 of 3");
  });

  it("Collapse folds every thought in the chat and Expand opens one", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    await waitFor(() => expect(api.getSettings).toHaveBeenCalled());
    const id = await startTurn("snap it");
    emit(id, { kind: "thought", text: "Need a snapshot." });
    emit(id, { kind: "tool", id: "t1", title: "read_file", status: "done", detail: "" });
    emit(id, { kind: "thought", text: "Now write." });
    emit(id, { kind: "text", text: "Done." });
    emit(id, { kind: "done", stop: "end_turn" });
    finish();

    expect(screen.getAllByText("Thought process")).toHaveLength(2);
    expect(screen.getByText("Need a snapshot.")).toBeVisible();
    expect(document.querySelector('[data-role="assistant"]')?.textContent).toBe("Done.");
    const [first] = screen.getAllByRole("button", { name: "Collapse" });
    expect(first).toHaveAttribute("aria-expanded", "true");
    await userEvent.click(first);

    expect(screen.queryByText("Need a snapshot.")).toBeNull();
    expect(screen.queryByText("Now write.")).toBeNull();
    expect(screen.queryByText("Thought process")).toBeNull();
    expect(screen.getAllByText("Thought")).toHaveLength(2);
    const expands = screen.getAllByRole("button", { name: "Expand" });
    expect(expands).toHaveLength(2);
    await userEvent.click(expands[0]);
    expect(screen.getByText("Need a snapshot.")).toBeVisible();
    expect(screen.queryByText("Now write.")).toBeNull();
    expect(screen.getAllByRole("button", { name: "Expand" })).toHaveLength(1);
    expect(screen.getByText("Done.")).toBeVisible();
    // Collapse on the one opened by hand folds it again with the rest.
    await userEvent.click(screen.getByRole("button", { name: "Collapse" }));
    expect(screen.queryByText("Need a snapshot.")).toBeNull();
    expect(screen.getAllByRole("button", { name: "Expand" })).toHaveLength(2);
  });

  it("after Collapse a new thought in that chat starts folded, and a fresh chat starts open", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    await waitFor(() => expect(api.getSettings).toHaveBeenCalled());
    const id = await startTurn("first");
    emit(id, { kind: "thought", text: "Plan one." });
    await userEvent.click(screen.getByRole("button", { name: "Collapse" }));
    emit(id, { kind: "tool", id: "t1", title: "read_file", status: "done", detail: "" });
    emit(id, { kind: "thought", text: "Plan two." });
    expect(screen.queryByText("Plan two.")).toBeNull();
    expect(screen.getAllByRole("button", { name: "Expand" })).toHaveLength(2);
    emit(id, { kind: "done", stop: "end_turn" });
    finish();

    api.sendChat.mockResolvedValue("end_turn");
    await waitFor(() => expect(screen.getByRole("button", { name: "New chat" })).toBeEnabled());
    await userEvent.click(screen.getByRole("button", { name: "New chat" }));
    holdTurn();
    const next = await startTurn("second");
    expect(next).not.toBe(id);
    emit(next, { kind: "thought", text: "Plan three." });
    expect(screen.getByText("Plan three.")).toBeVisible();
    expect(screen.getByRole("button", { name: "Collapse" })).toBeVisible();
  });

  it("with Always collapse on, a thought starts as one Thought row and the reply still shows", async () => {
    api.getSettings.mockResolvedValue({ ...SETTINGS, alwaysCollapseThoughts: true });
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    await waitFor(() => expect(api.getSettings).toHaveBeenCalled());
    const id = await startTurn("snap it");
    emit(id, { kind: "thought", text: "Need a snapshot." });
    emit(id, { kind: "text", text: "Done." });
    emit(id, { kind: "done", stop: "end_turn" });
    finish();
    await waitFor(() => expect(screen.queryByText("Need a snapshot.")).toBeNull());
    expect(screen.getByText("Thought")).toBeVisible();
    expect(screen.getByRole("button", { name: "Expand" })).toHaveAttribute("aria-expanded", "false");
    expect(screen.getByText("Done.")).toBeVisible();
  });

  it("a plan card's chips fill its reply box or answer it, and picks are remembered", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("plan it");
    emit(id, {
      kind: "elicit",
      id: "plan-1",
      server: "Plan",
      message: "1. Read the file",
      mode: "form",
      url: "",
      fieldName: "reply",
      fieldTitle: "Reply",
      secret: false,
      options: [],
      chips: [
        { key: "plan:go", label: "Yes, go", act: "send", text: "Go ahead with the plan." },
        { key: "plan:change", label: "Change this", act: "prefill", text: "Change the plan: " },
      ],
    });
    await userEvent.click(screen.getByRole("button", { name: "Change this" }));
    expect(screen.getByLabelText("Reply")).toHaveValue("Change the plan: ");
    expect(api.answerElicit).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Yes, go" }));
    await waitFor(() =>
      expect(api.answerElicit).toHaveBeenCalledWith(id, "plan-1", "accept", "reply", "Go ahead with the plan."),
    );
    await waitFor(() => expect(api.replyPick).toHaveBeenCalledWith("plan:go"));
    finish();
  });

  it("opening the chat that is already open keeps what is typed in the box", async () => {
    api.sendChat.mockResolvedValue("end_turn");
    const { rerender } = render(<Chat onOpenSettings={() => {}} remote />);
    const id = await startTurn("hi");
    await waitFor(() => expect(screen.queryByRole("button", { name: "Stop" })).toBeNull());
    const box = screen.getByRole("textbox", { name: "Message" });
    await userEvent.type(box, "my half-written next message");

    rerender(<Chat onOpenSettings={() => {}} remote prefill={{ text: "", n: 1, conversationId: id }} />);
    expect(box).toHaveValue("my half-written next message");
    expect(screen.getByText("hi")).toBeVisible();
    expect(api.closeChat).not.toHaveBeenCalled();

    rerender(<Chat onOpenSettings={() => {}} remote prefill={{ text: "", n: 2, conversationId: "shell-other" }} />);
    expect(box).toHaveValue("");
    expect(screen.queryByText("hi")).toBeNull();
  });

  it("a mission prefill moves to its chat once, then stays there", async () => {
    api.sendChat.mockResolvedValue("end_turn");
    const { rerender } = render(<Chat onOpenSettings={() => {}} />);
    const first = await startTurn("hello");
    await waitFor(() => expect(screen.getByLabelText("Message")).toBeEnabled());

    const mission = { text: "Mission from my decision journal: Q", n: 1, conversationId: "shell-m1" };
    rerender(<Chat onOpenSettings={() => {}} prefill={mission} />);
    expect(api.closeChat).toHaveBeenCalledWith(first);
    expect(screen.queryByText("hello")).toBeNull();
    expect(screen.getByLabelText("Message")).toHaveValue("Mission from my decision journal: Q");

    await userEvent.type(screen.getByLabelText("Message"), "{Enter}");
    await waitFor(() =>
      expect(api.sendChat).toHaveBeenLastCalledWith("shell-m1", "Mission from my decision journal: Q", undefined),
    );

    rerender(<Chat onOpenSettings={() => {}} prefill={{ ...mission, text: "again", n: 2 }} />);
    expect(api.closeChat).toHaveBeenCalledTimes(1);
    expect(screen.getByText("Mission from my decision journal: Q")).toHaveAttribute("data-role", "user");
    expect(screen.getByLabelText("Message")).toHaveValue("again");
  });

  it("a mission prefill waits for a running reply", async () => {
    const finish = holdTurn();
    const { rerender } = render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("hello");
    rerender(
      <Chat onOpenSettings={() => {}} prefill={{ text: "Mission", n: 1, conversationId: "shell-m1" }} />,
    );
    expect(screen.getByText(MISSION_BUSY)).toBeVisible();
    expect(screen.getByLabelText("Message")).toHaveValue("");
    expect(api.closeChat).not.toHaveBeenCalled();
    expect(screen.getByText("hello")).toBeVisible();
    finish();
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledWith(id, "hello", undefined));
  });

  it("a goal the reply noticed is asked about under it, and Track puts it on My goals", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("fishing is so relaxing");
    emit(id, { kind: "text", text: "Nice, go for 99!" });
    emit(id, { kind: "goal", id: "g7", title: "Old School RuneScape · 99 fishing" });
    emit(id, { kind: "done", stop: "end_turn" });
    finish();
    const ask = await screen.findByRole("group", { name: "Track this goal?" });
    expect(ask).toHaveTextContent("Old School RuneScape · 99 fishing");
    await userEvent.click(within(ask).getByRole("button", { name: "Track" }));
    expect(api.goalAct).toHaveBeenCalledWith([{ act: "track", id: "g7" }]);
    expect(await screen.findByText(ASK_TRACKED)).toBeInTheDocument();
  });

  it("a reply's follow-ups join the row under the box, and a tap sends one in its reply's mode", async () => {
    let finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("the build failed");
    emit(id, { kind: "text", text: "TypeError in parse_row." });
    expect(screen.queryByRole("group", { name: "Suggestions" })).toBeNull();
    api.quickChips.mockResolvedValue([
      chip("reply-0", "Fix the cause", "Fix the root cause in parse_row.", { mode: "plan" }),
      chip("reply-1", "Add a test", "Add a test for parse_row."),
      chip("pred-1", "Explain it", "Explain the error."),
      chip("pred-2", "Fourth", "Never shown."),
    ]);
    emit(id, { kind: "suggestions", items: [] });
    emit(id, { kind: "done", stop: "end_turn" });
    finish();
    const row = await screen.findByRole("group", { name: "Suggestions" });
    expect(api.quickChips).toHaveBeenLastCalledWith("composer", id, "", undefined);
    const taps = () =>
      within(row)
        .getAllByRole("button")
        .filter((b) => !b.getAttribute("aria-label")?.startsWith("Hide"));
    expect(taps().map((b) => b.textContent)).toEqual(["Fix the cause", "Add a test", "Explain it"]);
    expect(taps()[0]).toHaveAttribute("title", "Why this? Suggested in the reply  (Alt+1)");
    finish = holdTurn();
    await userEvent.click(within(row).getByRole("button", { name: "Fix the cause" }));
    expect(api.chipOutcome).toHaveBeenCalledWith("composer", "reply-0", true);
    await waitFor(() =>
      expect(api.sendChat).toHaveBeenLastCalledWith(id, "Fix the root cause in parse_row.", "plan"),
    );
    expect(screen.getByText("Fix the root cause in parse_row.")).toHaveAttribute("data-role", "user");
    expect(screen.queryByRole("group", { name: "Suggestions" })).toBeNull();
    finish();
  });

  it("a chip adds to what is typed, Go opens its page, and × hides one", async () => {
    const onOpenView = vi.fn();
    api.quickChips.mockResolvedValue([
      chip("pred-1", "Explain it", "Explain the error."),
      { ...chip("nav-1", "Automations", "__nav:night"), action: "go", view: "automations" },
    ]);
    render(<Chat onOpenSettings={() => {}} onOpenView={onOpenView} />);
    const row = await screen.findByRole("group", { name: "Suggestions" });
    await userEvent.type(screen.getByLabelText("Message"), "half typed");
    await userEvent.click(within(row).getByRole("button", { name: "Explain it" }));
    expect(screen.getByLabelText("Message")).toHaveValue("half typed Explain the error.");
    expect(api.sendChat).not.toHaveBeenCalled();

    await waitFor(() => expect(screen.getByRole("button", { name: "Automations" })).toBeVisible());
    await userEvent.click(screen.getByRole("button", { name: "Automations" }));
    expect(onOpenView).toHaveBeenCalledWith("automations");

    await userEvent.click(await screen.findByRole("button", { name: "Hide Explain it" }));
    expect(api.chipOutcome).toHaveBeenLastCalledWith("composer", "pred-1", false);
  });

  it("an empty chat shows the device glance, and a tap opens Devices", async () => {
    const onOpenView = vi.fn();
    api.getDeviceGlance.mockResolvedValue({ sharing: true, frame: null });
    render(<Chat onOpenSettings={() => {}} onOpenView={onOpenView} />);
    await userEvent.click(await screen.findByRole("button", { name: "Sharing. Open Devices" }));
    expect(onOpenView).toHaveBeenCalledWith("devices");
    // Once the chat has a message, the empty view and its glance go.
    api.sendChat.mockResolvedValue("end_turn");
    await userEvent.type(screen.getByLabelText("Message"), "hi{Enter}");
    await waitFor(() => expect(screen.queryByRole("button", { name: "Sharing. Open Devices" })).toBeNull());
  });

  it("Alt+2 taps the second chip", async () => {
    const onOpenView = vi.fn();
    api.quickChips.mockResolvedValue([
      chip("pred-1", "Explain it", "Explain the error."),
      { ...chip("nav-1", "Automations", "__nav:night"), action: "go", view: "automations" },
    ]);
    render(<Chat onOpenSettings={() => {}} onOpenView={onOpenView} />);
    await screen.findByRole("group", { name: "Suggestions" });
    fireEvent.keyDown(window, { key: "™", code: "Digit2", altKey: true });
    expect(onOpenView).toHaveBeenCalledWith("automations");
    expect(api.chipOutcome).toHaveBeenCalledWith("composer", "nav-1", true);
  });

  it("a reply without follow-ups and an empty row show no chips", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("list the files");
    emit(id, { kind: "text", text: "There are 12 files." });
    emit(id, { kind: "done", stop: "end_turn" });
    finish();
    await waitFor(() => expect(screen.getByLabelText("Message")).toBeEnabled());
    expect(screen.getByText("There are 12 files.")).toBeVisible();
    expect(screen.queryByRole("group", { name: "Suggestions" })).toBeNull();
  });

  it("/scratch opens a scratch chat that says it reads and saves no memory", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const first = await startTurn("hello");
    await act(async () => finish());
    expect(screen.queryByText("Scratch chat · no memory read or saved")).toBeNull();
    api.runSlash.mockReset().mockResolvedValue({ kind: "newScratch" });
    await userEvent.type(screen.getByLabelText("Message"), "/scratch{Enter}");
    expect(
      await screen.findByText("Scratch chat: it doesn't read your memory, and nothing said here is saved to it."),
    ).toBeInTheDocument();
    expect(screen.queryByText("hello")).toBeNull();
    api.sendChat.mockResolvedValue("end_turn");
    await userEvent.type(screen.getByLabelText("Message"), "just thinking out loud{Enter}");
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(2));
    const id = api.sendChat.mock.calls[1][0];
    expect(id).toMatch(/^shell-scratch-[a-z0-9]+-[a-z0-9]+$/);
    expect(first).toMatch(/^shell-(?!scratch-)/);
    // The mark stays while the chat is open, not only as the first note.
    expect(screen.getByText("Scratch chat · no memory read or saved")).toHaveClass("scratch-mark");
  });

  it("only a scratch chat id is a scratch chat", () => {
    expect(isScratchChat(newConversationId(true))).toBe(true);
    expect(isScratchChat(newConversationId())).toBe(false);
    expect(isScratchChat("shell-m3x9-scratch-")).toBe(false);
  });

  it("/deep-research sends the line as typed and runs no second command", async () => {
    api.runSlash.mockReset().mockResolvedValue({ kind: "queue", text: "/deep-research harbor tides" });
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "/deep-research harbor tides{Enter}");
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(1));
    expect(api.runSlash).toHaveBeenCalledTimes(1);
    expect(api.runSlash.mock.calls[0][0]).toBe("/deep-research harbor tides");
    expect(api.sendChat.mock.calls[0][1]).toBe("/deep-research harbor tides");
    expect(api.sendChat.mock.calls[0][2]).toBe("chat");
    expect(await screen.findByText("/deep-research harbor tides")).toBeInTheDocument();
  });

  it("/export saves this chat and leaves a card that opens or shows the file", async () => {
    api.runSlash.mockReset().mockResolvedValue({ kind: "export" });
    api.exportChat.mockReset().mockResolvedValue({
      token: "saved-1",
      name: "hello-20261106-1415.md",
      folder: "Chats",
      path: "/home/user/Documents/Loadout/Chats/hello-20261106-1415.md",
      line: "Saved hello-20261106-1415.md to Chats",
      revealLabel: "Show in folder",
    });
    api.revealSaved.mockReset().mockResolvedValue(undefined);
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("hello");
    await act(async () => finish());
    await userEvent.type(screen.getByLabelText("Message"), "/export{Enter}");
    expect(api.runSlash).toHaveBeenCalledWith("/export", id);
    expect(api.exportChat).toHaveBeenCalledWith(id);
    const card = await screen.findByRole("status", { name: "Saved hello-20261106-1415.md to Chats" });
    await userEvent.click(within(card).getByRole("button", { name: "Show in folder" }));
    expect(api.revealSaved).toHaveBeenCalledWith("saved-1");
    expect(api.sendChat).toHaveBeenCalledTimes(1);
  });

  it("/export html and /export json ask for that format", async () => {
    api.runSlash.mockReset().mockResolvedValue({ kind: "exportAs", format: "html" });
    api.exportChat.mockReset().mockResolvedValue({
      token: "saved-2",
      name: "hello-20261106-1415.html",
      folder: "Chats",
      path: "/home/user/Documents/Loadout/Chats/hello-20261106-1415.html",
      line: "Saved hello-20261106-1415.html to Chats",
      revealLabel: "Show in folder",
    });
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "/export html{Enter}");
    expect(
      await screen.findByRole("status", { name: "Saved hello-20261106-1415.html to Chats" }),
    ).toBeInTheDocument();
    expect(api.exportChat).toHaveBeenCalledWith(expect.stringMatching(/^shell-/), "html");
    api.runSlash.mockResolvedValue({ kind: "exportAs", format: "json" });
    await userEvent.type(screen.getByLabelText("Message"), "/export json{Enter}");
    await waitFor(() =>
      expect(api.exportChat).toHaveBeenLastCalledWith(expect.stringMatching(/^shell-/), "json"),
    );
  });

  it("a failed /export says why and saves no card", async () => {
    api.runSlash.mockReset().mockResolvedValue({ kind: "export" });
    api.exportChat.mockReset().mockRejectedValue("There's nothing in this chat to save yet.");
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "/export{Enter}");
    expect(await screen.findByText("There's nothing in this chat to save yet.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Show in folder" })).toBeNull();
  });

  it("/import with no OpenClaw workspace to read says so in the chat and opens nothing", async () => {
    api.runSlash.mockReset().mockResolvedValue({ kind: "import" });
    const opened = vi.fn();
    window.addEventListener("onboard:open", opened);
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "/import{Enter}");
    expect(await screen.findByText("No OpenClaw or Hermes workspace on this computer.")).toBeInTheDocument();
    expect(opened).not.toHaveBeenCalled();
    window.removeEventListener("onboard:open", opened);
    expect(api.sendChat).not.toHaveBeenCalled();
  });

  it("sends the text and streams its own conversation's reply", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("hello");

    expect(id).toMatch(/^shell-/);
    expect(api.sendChat).toHaveBeenCalledWith(id, "hello", undefined);
    expect(screen.getByText("hello")).toHaveAttribute("data-role", "user");
    expect(screen.getByLabelText("Message")).toHaveValue("");

    emit("someone-else", { kind: "text", text: "nope" });
    emit(id, { kind: "text", text: "Hi from " });
    emit(id, { kind: "text", text: "Grok" });
    // The reply renders as Markdown, so its text sits in a paragraph inside the bubble.
    const bubble = () => screen.getByText("Hi from Grok").closest(".bubble");
    expect(bubble()).toHaveClass("streaming");
    expect(screen.queryByText(/nope/)).toBeNull();

    finish();
    await waitFor(() => expect(bubble()).not.toHaveClass("streaming"));
    expect(bubble()).toHaveAttribute("data-role", "assistant");
  });

  it("answers a setting ask here without sending it to the model", async () => {
    auto.settingAsk.mockResolvedValueOnce("Text size is 115% now.");
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "make the text bigger{Enter}");
    expect(await screen.findByText("Text size is 115% now.")).toBeInTheDocument();
    expect(auto.settingAsk).toHaveBeenCalledWith("make the text bigger");
    expect(screen.getByText("make the text bigger")).toHaveAttribute("data-role", "user");
    expect(screen.getByLabelText("Message")).toHaveValue("");
    expect(api.sendChat).not.toHaveBeenCalled();
  });

  it("renders the reply as Markdown and keeps the person's message plain", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("**mine** <b>x</b>");
    emit(id, { kind: "text", text: "## Plan\n- **one**\n<script>alert(1)</script>" });
    finish();
    const reply = await screen.findByRole("heading", { level: 2, name: "Plan" });
    const bubble = reply.closest(".bubble")!;
    expect(bubble).toHaveAttribute("data-role", "assistant");
    expect(within(bubble as HTMLElement).getByRole("listitem")).toHaveTextContent("one");
    expect(bubble.querySelector("strong")).toHaveTextContent("one");
    expect(bubble.querySelector("script")).toBeNull();
    expect(within(bubble as HTMLElement).getByText("<script>alert(1)</script>")).toBeInTheDocument();
    const mine = screen.getByText("**mine** <b>x</b>");
    expect(mine).toHaveAttribute("data-role", "user");
    expect(mine.querySelector("strong, b")).toBeNull();
  });

  it("shows a permission card and Allow once answers it", async () => {
    holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("write a note");

    emit(id, {
      kind: "permission",
      id: "call-1",
      title: "write",
      action: "note.txt",
      reason: "",
    });
    const card = screen.getByRole("group", { name: "Approve write" });
    expect(card).toHaveTextContent("Loadout wants to run write");
    expect(card).toHaveTextContent("note.txt");

    await userEvent.click(screen.getByRole("button", { name: "Allow once" }));
    expect(api.answerPermission).toHaveBeenCalledWith(id, "call-1", "allow");
    expect(card).toHaveTextContent("Allowed once");
    expect(screen.queryByRole("button", { name: "Allow once" })).toBeNull();
  });

  it("an answer that doesn't reach the agent gives the buttons back", async () => {
    holdTurn();
    api.answerPermission.mockRejectedValueOnce("Can't reach Loadout on your computer. Is it open?");
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("write a note");

    emit(id, { kind: "permission", id: "call-9", title: "write", action: "note.txt", reason: "" });
    await userEvent.click(screen.getByRole("button", { name: "Allow once" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("Can't reach Loadout on your computer.");
    const card = screen.getByRole("group", { name: "Approve write" });
    expect(card).not.toHaveTextContent("Allowed once");
    await userEvent.click(within(card).getByRole("button", { name: "Allow once" }));
    expect(api.answerPermission).toHaveBeenLastCalledWith(id, "call-9", "allow");
    expect(card).toHaveTextContent("Allowed once");
  });

  it("Deny and Always allow send their answers", async () => {
    holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("go");

    emit(id, { kind: "permission", id: "a", title: "write", action: "a.txt", reason: "" });
    await userEvent.click(screen.getByRole("button", { name: "Deny" }));
    expect(api.answerPermission).toHaveBeenLastCalledWith(id, "a", "deny");
    expect(screen.getByText("Denied")).toBeInTheDocument();

    emit(id, {
      kind: "permission",
      id: "b",
      title: "run_terminal_command",
      action: "ls",
      reason: "",
    });
    await userEvent.click(screen.getByRole("button", { name: "Always allow" }));
    expect(api.answerPermission).toHaveBeenLastCalledWith(id, "b", "always");
    expect(screen.getByText("Always allowed")).toBeInTheDocument();
  });

  it("a merge request's card says what it is and offers only Allow once and Deny", async () => {
    holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("merge it");

    emit(id, {
      kind: "permission",
      id: "m1",
      title: "merge_subagent_work",
      action: "3 files (1 new, 2 changed, 0 deleted) into /home/user/my-app: Fix the parser",
      reason: "",
    });
    const card = screen.getByRole("group", { name: "Approve merge_subagent_work" });
    expect(card.textContent).toBe(
      "Loadout wants to merge a helper's work into the project" +
        "3 files (1 new, 2 changed, 0 deleted) into /home/user/my-app: Fix the parser" +
        "Allow onceDeny",
    );
    expect(within(card).getAllByRole("button").map((button) => button.textContent)).toEqual(["Allow once", "Deny"]);

    await userEvent.click(within(card).getByRole("button", { name: "Allow once" }));
    expect(api.answerPermission).toHaveBeenLastCalledWith(id, "m1", "allow");
  });

  it("a card nobody answered is retired when the turn ends", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("write it");

    emit(id, { kind: "permission", id: "c", title: "write", action: "c.txt", reason: "" });
    await userEvent.click(screen.getByRole("button", { name: "Stop" }));
    finish();

    expect(await screen.findByText("Not run: the turn ended")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Allow once" })).toBeNull();
    expect(api.answerPermission).not.toHaveBeenCalled();
  });

  it("Halt retires every waiting card as halted and says so once", async () => {
    let finish: (stop: string) => void = () => {};
    api.sendChat.mockImplementation(() => new Promise<string>((resolve) => (finish = resolve)));
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("type it");

    emit(id, { kind: "permission", id: "d", title: "type", action: "hello", reason: "" });
    emit(id, {
      kind: "elicit",
      id: "ask-h",
      server: "Question",
      message: "Which window?",
      mode: "form",
      url: "",
      fieldName: "answer",
      fieldTitle: "Your answer",
      secret: false,
    });
    emit(id, { kind: "done", stop: "halted" });
    await act(async () => finish("halted"));

    expect(screen.getByText("Not run: halted")).toBeInTheDocument();
    expect(screen.getByText("Not answered: halted.")).toBeInTheDocument();
    expect(screen.getAllByText(HALTED_LINE)).toHaveLength(1);
    expect(screen.getByText(HALTED_LINE)).toHaveAttribute("role", "status");
    expect(screen.queryByRole("button", { name: "Allow once" })).toBeNull();
    expect(api.answerPermission).not.toHaveBeenCalled();
  });

  it("a turn that comes back halted settles its cards even if the done event was missed", async () => {
    let finish: (stop: string) => void = () => {};
    api.sendChat.mockImplementation(() => new Promise<string>((resolve) => (finish = resolve)));
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("type it");

    emit(id, { kind: "permission", id: "e", title: "type", action: "hi", reason: "" });
    await act(async () => finish("halted"));

    expect(screen.getByText("Not run: halted")).toBeInTheDocument();
    expect(screen.getByText(HALTED_LINE)).toBeInTheDocument();
    expect(screen.queryByText("Not run: the turn ended")).toBeNull();
  });

  it("a Deny from a notification shows on its card", async () => {
    holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("write both");

    emit(id, { kind: "permission", id: "n1", title: "write", action: "n.txt", reason: "" });
    emit(id, { kind: "permission", id: "n2", title: "edit", action: "m.txt", reason: "" });
    emit(id, { kind: "answered", id: "n1", answer: "deny" });

    expect(screen.getByRole("group", { name: "Approve write" })).toHaveTextContent("Denied");
    const other = screen.getByRole("group", { name: "Approve edit" });
    expect(other).not.toHaveTextContent("Denied");
    expect(screen.getAllByRole("button", { name: "Allow once" })).toHaveLength(1);
    expect(api.answerPermission).not.toHaveBeenCalled();
  });

  it("an answer that comes after the turn ended leaves the card ended", () => {
    const ended: Item[] = [
      { kind: "permission", id: "a", title: "write", action: "", reason: "", answer: "ended" },
      { kind: "permission", id: "b", title: "write", action: "", reason: "", answer: "allow" },
    ];
    expect(applyEvent(ended, { kind: "answered", id: "a", answer: "deny" })).toEqual(ended);
    const halted: Item[] = [{ kind: "permission", id: "h", title: "type", action: "", reason: "", answer: "halted" }];
    expect(applyEvent(halted, { kind: "answered", id: "h", answer: "deny" })).toEqual(halted);
    expect(applyEvent(ended, { kind: "answered", id: "b", answer: "deny" })[1]).toMatchObject({
      answer: "deny",
    });
  });

  it("a reply that hasn't started shows a thinking orb, and a lookup tool a searching one", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("what's new");

    const orb = screen.getByRole("img", { name: "Thinking" });
    expect(orb.tagName).toBe("CANVAS");
    expect(orb.closest(".bubble")).toHaveAttribute("data-role", "assistant");
    expect(screen.queryByText("…")).toBeNull();

    emit(id, { kind: "tool", id: "t", title: "web_search", status: "pending", detail: "" });
    expect(screen.getByRole("img", { name: "Looking it up" })).toBeInTheDocument();
    expect(screen.queryByRole("img", { name: "Thinking" })).toBeNull();

    emit(id, { kind: "text", text: "Here" });
    expect(screen.queryByRole("img", { name: "Looking it up" })).toBeNull();
    finish();
  });

  it("Stop cancels the turn in flight", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("count");

    await userEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(api.cancelChat).toHaveBeenCalledWith(id);

    finish();
    await waitFor(() => expect(screen.getByRole("button", { name: "Send" })).toBeDisabled());
  });

  it("ignores an event that arrives after the turn ended", async () => {
    api.sendChat.mockResolvedValue("end_turn");
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("hi");
    await waitFor(() => expect(screen.getByRole("button", { name: "Send" })).toBeDisabled());

    emit(id, { kind: "text", text: "late" });
    emit(id, { kind: "permission", id: "x", title: "write", action: "x", reason: "" });

    expect(screen.queryByText("late")).toBeNull();
    expect(screen.queryByRole("group", { name: "Approve write" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Stop" })).toBeNull();
  });

  it("shows a helper's question after the turn ended, once, and sends its answer", async () => {
    api.sendChat.mockResolvedValue("end_turn");
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("look into it");
    await waitFor(() => expect(screen.getByRole("button", { name: "Send" })).toBeDisabled());

    const ask: ChatEvent = { kind: "permission", id: "h1", title: "write", action: "notes.md", reason: "" };
    act(() => api.handler?.({ conversationId: id, event: ask, helper: true }));
    act(() => api.handler?.({ conversationId: id, event: ask, helper: true }));
    act(() => api.handler?.({ conversationId: "another-chat", event: { ...ask, id: "h2" }, helper: true }));

    expect(screen.getAllByRole("group", { name: "Approve write" })).toHaveLength(1);
    await userEvent.click(screen.getByRole("button", { name: "Allow once" }));
    expect(api.answerPermission).toHaveBeenLastCalledWith(id, "h1", "allow");
  });

  it("a turn's end retires its own cards but leaves a helper's question open", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("go");

    emit(id, { kind: "permission", id: "own", title: "write", action: "a.txt", reason: "" });
    act(() =>
      api.handler?.({
        conversationId: id,
        event: { kind: "permission", id: "h", title: "delete", action: "b.txt", reason: "" },
        helper: true,
      }),
    );
    act(() => finish());

    const own = screen.getByRole("group", { name: "Approve write" });
    await waitFor(() => expect(own).toHaveTextContent("Not run: the turn ended"));
    const helper = screen.getByRole("group", { name: "Approve delete" });
    expect(within(helper).getByRole("button", { name: "Allow once" })).toBeInTheDocument();
  });

  it("shows the error and links to Settings", async () => {
    api.sendChat.mockRejectedValue("Add an xAI API key in Settings first.");
    const openSettings = vi.fn();
    render(<Chat onOpenSettings={openSettings} />);

    await userEvent.type(screen.getByLabelText("Message"), "hi{Enter}");

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Add an xAI API key in Settings first.",
    );
    await userEvent.click(screen.getByRole("button", { name: "Open Settings" }));
    expect(openSettings).toHaveBeenCalledTimes(1);
  });

  it("New chat closes the old conversation and starts a fresh one", async () => {
    api.sendChat.mockResolvedValue("end_turn");
    render(<Chat onOpenSettings={() => {}} />);
    const first = await startTurn("one");
    const fresh = () =>
      screen.getByRole("button", { name: "New chat" });
    await waitFor(() => expect(fresh()).toBeEnabled());

    await userEvent.click(fresh());
    expect(api.closeChat).toHaveBeenCalledWith(first);
    expect(screen.queryByText("one")).toBeNull();

    const second = await startTurn("two");
    expect(second).not.toBe(first);
  });

  it("New chat still starts a fresh chat when the app refuses to close the old one, with no stray rejection", async () => {
    const unhandled = vi.fn<(reason: unknown) => void>();
    // The test runner's Node process reports a rejection nothing handled.
    type Rejections = Record<"on" | "off", (event: "unhandledRejection", listener: (reason: unknown) => void) => void>;
    const node = (globalThis as unknown as { process: Rejections }).process;
    node.on("unhandledRejection", unhandled);
    try {
      // A paired browser's chat with no message sent: the computer refuses the close.
      // The mock library handles a rejected promise it returns; this one is
      // left for the page to handle, as a real call's is.
      api.closeChat.mockImplementation(() => {
        const real = Promise.reject(new Error("That chat isn't open in this browser."));
        return { then: real.then.bind(real), catch: real.catch.bind(real) } as unknown as Promise<void>;
      });
      api.sendChat.mockResolvedValue("end_turn");
      render(<Chat onOpenSettings={() => {}} remote />);
      await userEvent.click(screen.getByRole("button", { name: "New chat" }));
      expect(api.closeChat).toHaveBeenCalledTimes(1);
      const refused = api.closeChat.mock.calls[0][0];
      // Node reports an unhandled rejection after the microtasks drain.
      await new Promise((resolve) => setTimeout(resolve, 0));
      expect(unhandled.mock.calls.map(([reason]) => String(reason))).toEqual([]);
      expect(await startTurn("hi")).not.toBe(refused);
    } finally {
      node.off("unhandledRejection", unhandled);
    }
  });

  it("the web app asks for a new chat's chips without the chat id, and with it once a message is sent", async () => {
    api.sendChat.mockResolvedValue("end_turn");
    api.quickChips.mockResolvedValue([chip("start", "Start here", "Start here")]);
    render(<Chat onOpenSettings={() => {}} remote />);
    const row = await screen.findByRole("group", { name: "Suggestions" });
    expect(within(row).getByRole("button", { name: "Start here" })).toBeVisible();
    // The computer refuses chips for a chat this browser has not sent in.
    expect(api.quickChips.mock.calls.map(([surface, id]) => [surface, id])).toEqual([["composer", undefined]]);

    const id = await startTurn("hi");
    emit(id, { kind: "done", stop: "end_turn" });
    await waitFor(() => expect(api.quickChips).toHaveBeenLastCalledWith("composer", id, "", undefined));
  });

  it("the desktop window asks for a new chat's chips with the chat id", async () => {
    render(<Chat onOpenSettings={() => {}} />);
    await waitFor(() => expect(api.quickChips).toHaveBeenCalled());
    expect(api.quickChips).toHaveBeenLastCalledWith("composer", expect.stringMatching(/^shell-/), "", undefined);
  });

  it("Shift+Enter adds a line instead of sending", async () => {
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "a{Shift>}{Enter}{/Shift}b");
    expect(screen.getByLabelText("Message")).toHaveValue("a\nb");
    expect(api.sendChat).not.toHaveBeenCalled();
  });

  it("the conversation log takes keyboard focus, so a long one can be scrolled", async () => {
    render(<Chat onOpenSettings={() => {}} />);
    // Hangout tab strip and TALK/TYPE sit above the log in tab order.
    screen.getByRole("log").focus();
    expect(screen.getByRole("log")).toHaveFocus();
  });
});

describe("applyEvent", () => {
  it("joins text, starts a new bubble after a tool, and updates a tool card in place", () => {
    let items: Item[] = [{ kind: "user", text: "go" }];
    items = applyEvent(items, { kind: "text", text: "Look" });
    items = applyEvent(items, { kind: "text", text: "ing." });
    items = applyEvent(items, {
      kind: "tool",
      id: "t",
      title: "read_file",
      status: "pending",
      detail: "",
    });
    items = applyEvent(items, {
      kind: "tool",
      id: "t",
      title: "read_file",
      status: "completed",
      detail: "a.txt",
    });
    items = applyEvent(items, { kind: "text", text: "Done." });
    expect(items).toEqual([
      { kind: "user", text: "go" },
      { kind: "assistant", text: "Looking.", streaming: true },
      { kind: "tool", id: "t", title: "read_file", status: "completed", detail: "a.txt" },
      { kind: "assistant", text: "Done.", streaming: true },
    ]);
  });

  it("the app picks how each message runs: there are no mode or approval chips", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    // The composer names the product, not the model.
    expect(screen.getByLabelText("Message")).toHaveAttribute("placeholder", "Message Loadout");
    for (const name of ["Chat", "Plan", "btw", "Ask", "Auto", "Always"]) {
      expect(screen.queryByRole("button", { name })).toBeNull();
    }
    const id = await startTurn("what does main.rs do");
    expect(api.sendChat).toHaveBeenCalledWith(id, "what does main.rs do", undefined);
    finish();
  });

  it("the composer shows no model or effort and has nothing to pick one with", async () => {
    render(<Chat onOpenSettings={() => {}} />);
    await waitFor(() => expect(api.getSettings).toHaveBeenCalled());
    expect(screen.queryByText(/effort/i)).toBeNull();
    expect(screen.queryByText(/grok-\d/i)).toBeNull();
    expect(screen.queryByText(/^model/i)).toBeNull();
    expect(screen.queryByLabelText(/model/i)).toBeNull();
    expect(screen.queryByRole("button", { name: /model|effort|grok/i })).toBeNull();
    expect(document.querySelector(".chips")).toBeNull();
  });

  it("Stop asking me allows the step and moves the setting to only ask for risky things", async () => {
    holdTurn();
    api.answerPermission.mockResolvedValue();
    api.saveSettings.mockResolvedValue({ ...SETTINGS, approvals: "auto" });
    render(<Chat onOpenSettings={() => {}} />);
    await waitFor(() => expect(api.getSettings).toHaveBeenCalled());
    const id = await startTurn("write a note");
    emit(id, { kind: "permission", id: "call-1", title: "write", action: "note.txt", reason: "" });
    await userEvent.click(screen.getByRole("button", { name: "Stop asking me" }));
    expect(api.answerPermission).toHaveBeenCalledWith(id, "call-1", "allow");
    expect(api.saveSettings).toHaveBeenCalledWith(true, "", "auto", false);
    expect(await screen.findByText(STOP_ASKING_DONE)).toBeVisible();
    emit(id, { kind: "permission", id: "call-2", title: "run", action: "make", reason: "" });
    // Only ask for risky things now: the next card has nothing looser to offer.
    expect(screen.queryByRole("button", { name: "Stop asking me" })).toBeNull();
  });

  it("Stop asking me keeps a setting that changed after the chat loaded", async () => {
    holdTurn();
    api.answerPermission.mockResolvedValue();
    api.saveSettings.mockResolvedValue({ ...SETTINGS, closeToTray: false, approvals: "auto" });
    render(<Chat onOpenSettings={() => {}} />);
    await waitFor(() => expect(api.getSettings).toHaveBeenCalled());
    const id = await startTurn("write a note");
    // Close to tray went off elsewhere while this chat stayed open.
    api.getSettings.mockResolvedValue({ ...SETTINGS, closeToTray: false });
    emit(id, { kind: "permission", id: "call-1", title: "write", action: "note.txt", reason: "" });
    await userEvent.click(screen.getByRole("button", { name: "Stop asking me" }));
    await waitFor(() => expect(api.saveSettings).toHaveBeenCalledWith(false, "", "auto", false));
    api.getSettings.mockResolvedValue(SETTINGS);
  });

  it("on the web app the permission card has no Stop asking me", async () => {
    holdTurn();
    render(<Chat onOpenSettings={() => {}} remote />);
    await waitFor(() => expect(api.getSettings).toHaveBeenCalled());
    const id = await startTurn("write a note");
    emit(id, { kind: "permission", id: "call-1", title: "write", action: "note.txt", reason: "" });
    expect(screen.getByRole("button", { name: "Allow once" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "Stop asking me" })).toBeNull();
  });

  it("a plan card approves, and the next message is read afresh", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("tidy the build");
    emit(id, { kind: "plan", text: "1. Read build.rs" });
    emit(id, {
      kind: "elicit",
      id: "plan-1",
      server: "Plan",
      message: "1. Read build.rs",
      mode: "form",
      url: "",
      fieldName: null,
      fieldTitle: "",
      secret: false,
    });
    const card = screen.getByRole("group", { name: "The plan" });
    expect(card).toHaveTextContent("1. Read build.rs");
    expect(screen.getAllByText("1. Read build.rs")).toHaveLength(1);
    expect(screen.queryByText("…")).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Approve plan" }));
    expect(api.answerElicit).toHaveBeenCalledWith(id, "plan-1", "accept", null, "");
    expect(card).toHaveTextContent("Approved. Loadout can make the changes now.");
    finish();
    await waitFor(() => expect(screen.getByRole("button", { name: "Send" })).toBeVisible());
    holdTurn();
    await startTurn("thanks");
    expect(api.sendChat).toHaveBeenLastCalledWith(id, "thanks", undefined);
  });

  it("/plan makes only the next message plan first", async () => {
    api.runSlash.mockResolvedValue({ kind: "mode", mode: "plan", text: null });
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "/plan{Enter}");
    expect(await screen.findByText(PLAN_NEXT)).toBeVisible();
    expect(api.sendChat).not.toHaveBeenCalled();
    let finish = holdTurn();
    const id = await startTurn("set up the release pipeline");
    expect(api.sendChat).toHaveBeenLastCalledWith(id, "set up the release pipeline", "plan");
    finish();
    await waitFor(() => expect(screen.getByRole("button", { name: "Send" })).toBeVisible());
    finish = holdTurn();
    await startTurn("and what about docs?");
    expect(api.sendChat).toHaveBeenLastCalledWith(id, "and what about docs?", undefined);
    finish();
  });

  it("a question takes a typed answer, and an unanswered one ends with the turn", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("pick a color");
    const ask = (n: number): ChatEvent => ({
      kind: "elicit",
      id: `ask-${n}`,
      server: "Question",
      message: `Which color? (${n})`,
      mode: "form",
      url: "",
      fieldName: "answer",
      fieldTitle: "Your answer",
      secret: false,
    });
    emit(id, ask(1));
    const send = screen.getByRole("button", { name: "Send" });
    expect(send).toBeDisabled();
    await userEvent.type(screen.getByLabelText("Your answer"), "blue{Enter}");
    expect(api.answerElicit).toHaveBeenCalledWith(id, "ask-1", "accept", "answer", "blue");
    expect(screen.getByRole("group", { name: "Loadout asks" })).toHaveTextContent("Sent.");
    emit(id, ask(2));
    finish();
    await waitFor(() =>
      expect(screen.getByText("Not answered: the turn ended.")).toBeInTheDocument(),
    );
  });

  it("signed out, it shows one sign-in button and a hint instead of the empty line", async () => {
    api.getSettings.mockResolvedValue({ ...SETTINGS, hasApiKey: false });
    const onSignIn = vi.fn();
    render(<Chat onOpenSettings={() => {}} onSignIn={onSignIn} />);
    const card = await screen.findByRole("region", { name: "Sign in" });
    expect(within(card).getAllByRole("button").map((b) => b.textContent)).toEqual([
      "Add your API key",
    ]);
    expect(card).toHaveTextContent(
      "Sign in to start" +
        "Loadout chats with your xAI API key. It's kept in this device's keychain, never in a file." +
        "Add your API key" +
        "No key yet? Make one at console.x.ai.",
    );
    expect(screen.queryByText("Ask Loadout anything.")).toBeNull();
    await userEvent.click(within(card).getByRole("button", { name: "Add your API key" }));
    expect(onSignIn).toHaveBeenCalledTimes(1);
  });

  it("where Grok sign-in is offered, the card leads with it, and a sign-in hides the card", async () => {
    api.getSettings.mockResolvedValue({ ...SETTINGS, hasApiKey: false });
    const onSignIn = vi.fn();
    const { unmount } = render(<Chat onOpenSettings={() => {}} onSignIn={onSignIn} grokSignIn />);
    const card = await screen.findByRole("region", { name: "Sign in" });
    expect(card).toHaveTextContent("Sign in with your Grok account, or add an xAI API key.");
    await userEvent.click(within(card).getByRole("button", { name: "Sign in with Grok" }));
    expect(onSignIn).toHaveBeenCalledTimes(1);
    unmount();
    api.getSettings.mockResolvedValue({
      ...SETTINGS,
      hasApiKey: false,
      signedIn: { name: "Sam", email: null },
    });
    render(<Chat onOpenSettings={() => {}} grokSignIn />);
    expect(await screen.findByText(CHAT_EMPTY)).toBeVisible();
    expect(screen.queryByRole("region", { name: "Sign in" })).toBeNull();
  });

  it("signed out on the web app, it says to sign in on the computer", async () => {
    api.getSettings.mockResolvedValue({ ...SETTINGS, hasApiKey: false });
    render(<Chat onOpenSettings={() => {}} remote />);
    expect(
      await screen.findByText(
        "Loadout on your computer isn't signed in yet. Sign in there, then chat here.",
      ),
    ).toBeVisible();
    expect(screen.queryByRole("region", { name: "Sign in" })).toBeNull();
  });
});

describe("Chat history", () => {
  beforeEach(() => {
    for (const fn of [api.sendChat, api.closeChat, api.cancelChat, api.loadChat]) fn.mockReset();
    api.closeChat.mockResolvedValue();
    api.getSettings.mockReset().mockResolvedValue(SETTINGS);
    api.getStartChat.mockReset().mockResolvedValue(null);
    api.handler = null;
  });

  it("a finished web-search reply shows its Sources as site chips that open the link", async () => {
    const open = vi.spyOn(window, "open").mockImplementation(() => null);
    try {
      const finish = holdTurn();
      render(<Chat onOpenSettings={() => {}} />);
      const id = await startTurn("what's new in rust?");
      emit(id, { kind: "text", text: "Rust 1.92 is out.\n\nSources:\n1. https://blog.rust-lang.org/2026/10/30/Rust-1.92.html\n" });
      // Still streaming: the list could keep growing, so it stays text.
      expect(screen.queryByRole("list", { name: "Sources" })).toBeNull();
      emit(id, { kind: "text", text: "2. https://www.phoronix.com/news/Rust-1.92\n" });
      emit(id, { kind: "done", stop: "end_turn" });
      finish();
      const chips = await screen.findByRole("list", { name: "Sources" });
      expect(within(chips).getAllByRole("button").map((chip) => chip.textContent)).toEqual([
        "blog.rust-lang.org",
        "phoronix.com",
      ]);
      const bubble = chips.closest('[data-role="assistant"]');
      expect(bubble).toHaveTextContent("Rust 1.92 is out.");
      expect(bubble).not.toHaveTextContent("https://www.phoronix.com");
      const phoronix = within(chips).getByRole("button", { name: "Open phoronix.com" });
      expect(phoronix).toHaveAttribute("title", "https://www.phoronix.com/news/Rust-1.92");
      await userEvent.click(phoronix);
      expect(open).toHaveBeenCalledWith("https://www.phoronix.com/news/Rust-1.92", "_blank", "noopener,noreferrer");
    } finally {
      open.mockRestore();
    }
  });

  it("a saved reply's Sources show as chips; a user line and a list with no heading stay text", async () => {
    api.getStartChat.mockResolvedValue({
      id: "shell-src",
      title: "rust",
      lines: [
        { role: "user", text: "Sources:\n- https://mine.example/a" },
        { role: "assistant", text: "Answer.\n- https://a.com" },
        { role: "assistant", text: "Repairable. Sources: https://frame.work/laptop16 https://www.dell.com/xps15" },
      ],
      earlier: 0,
    });
    render(<Chat onOpenSettings={() => {}} />);
    const chips = await screen.findByRole("list", { name: "Sources" });
    expect(within(chips).getAllByRole("button").map((chip) => chip.textContent)).toEqual(["frame.work", "dell.com"]);
    expect(chips.closest('[data-role="assistant"]')).toHaveTextContent(/^Repairable\.\s*frame\.workdell\.com/);
    expect(screen.getAllByRole("list", { name: "Sources" })).toHaveLength(1);
    expect(screen.getByText("https://a.com")).toBeVisible();
    expect(document.querySelector('[data-role="user"]')).toHaveTextContent("https://mine.example/a");
  });

  it("turns a saved chat's lines into bubbles and one muted tools line", () => {
    expect(itemsFrom(SAVED)).toEqual([
      { kind: "user", text: "pick a paint colour" },
      { kind: "tools", text: "read_file, web_search" },
      { kind: "assistant", text: "Try **blue**.", streaming: false },
    ]);
  });

  it("reopens the last chat on mount, and the next message carries it on", async () => {
    api.getStartChat.mockResolvedValue(SAVED);
    api.sendChat.mockResolvedValue("end_turn");
    render(<Chat onOpenSettings={() => {}} />);
    expect(await screen.findByText("pick a paint colour")).toHaveAttribute("data-role", "user");
    expect(screen.getByText("Tools ran: read_file, web_search")).toHaveAttribute("data-role", "tools");
    expect(screen.getByText("blue").closest("[data-role]")).toHaveAttribute("data-role", "assistant");
    await startTurn("and the trim?");
    expect(api.sendChat).toHaveBeenLastCalledWith("shell-saved", "and the trim?", undefined);
  });

  it("the web app never asks for the desktop's last chat", async () => {
    render(<Chat onOpenSettings={() => {}} remote />);
    await waitFor(() => expect(api.getSettings).toHaveBeenCalled());
    expect(api.getStartChat).not.toHaveBeenCalled();
  });

  it("a last chat that arrives after the person started typing away stays out", async () => {
    let arrive: (t: Transcript) => void = () => {};
    api.getStartChat.mockReturnValue(new Promise((resolve) => (arrive = resolve)));
    api.sendChat.mockResolvedValue("end_turn");
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("fresh question");
    await act(async () => arrive(SAVED));
    expect(screen.queryByText("pick a paint colour")).toBeNull();
    expect(id).not.toBe("shell-saved");
  });

  it("a chat opened from History replaces the open one, which stays saved", async () => {
    api.sendChat.mockResolvedValue("end_turn");
    api.loadChat.mockResolvedValue(SAVED);
    const { rerender } = render(<Chat onOpenSettings={() => {}} />);
    const first = await startTurn("hello");
    await waitFor(() => expect(screen.getByLabelText("Message")).toBeEnabled());

    rerender(<Chat onOpenSettings={() => {}} open={{ id: "shell-saved", n: 1 }} />);
    expect(await screen.findByText("pick a paint colour")).toBeVisible();
    expect(api.closeChat).toHaveBeenCalledWith(first);
    expect(api.loadChat).toHaveBeenCalledWith("shell-saved");
    expect(screen.queryByText("hello")).toBeNull();

    // Picking the chat that is already open does nothing.
    rerender(<Chat onOpenSettings={() => {}} open={{ id: "shell-saved", n: 2 }} />);
    expect(api.loadChat).toHaveBeenCalledTimes(1);
    expect(api.closeChat).toHaveBeenCalledTimes(1);
  });

  it("opening a chat waits for a running reply", async () => {
    const finish = holdTurn();
    const { rerender } = render(<Chat onOpenSettings={() => {}} />);
    await startTurn("hello");
    rerender(<Chat onOpenSettings={() => {}} open={{ id: "shell-saved", n: 1 }} />);
    expect(screen.getByText(OPEN_BUSY)).toBeVisible();
    expect(api.loadChat).not.toHaveBeenCalled();
    expect(api.closeChat).not.toHaveBeenCalled();
    finish();
  });

  it("a chat picked after the reply's done opens as soon as the reply settles, with no error", async () => {
    const finish = holdTurn();
    api.loadChat.mockResolvedValue(SAVED);
    const { rerender } = render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("test");
    emit(id, { kind: "text", text: "Test back at ya. What's good?" });
    emit(id, { kind: "done", stop: "end_turn" });
    rerender(<Chat onOpenSettings={() => {}} open={{ id: "shell-saved", n: 1 }} />);
    expect(screen.queryByText(OPEN_BUSY)).toBeNull();
    // The app's wrap-up after done finishes before the old chat closes.
    expect(api.closeChat).not.toHaveBeenCalled();
    await act(async () => finish());
    expect(await screen.findByText("pick a paint colour")).toBeVisible();
    expect(api.closeChat).toHaveBeenCalledWith(id);
    expect(api.loadChat).toHaveBeenCalledWith("shell-saved");
    expect(screen.queryByText("Test back at ya. What's good?")).toBeNull();
    expect(screen.queryByText(OPEN_BUSY)).toBeNull();
  });

  it("a failed reply's error stays with its chat when another chat was picked after done", async () => {
    let fail: (why: string) => void = () => {};
    api.sendChat.mockImplementation(() => new Promise<string>((_, reject) => (fail = reject)));
    api.loadChat.mockResolvedValue(SAVED);
    const { rerender } = render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("test");
    emit(id, { kind: "error", message: "The model stopped answering." });
    emit(id, { kind: "done", stop: "error" });
    rerender(<Chat onOpenSettings={() => {}} open={{ id: "shell-saved", n: 1 }} />);
    await act(async () => fail("The model stopped answering."));
    expect(await screen.findByText("pick a paint colour")).toBeVisible();
    expect(screen.queryByText(/The model stopped answering/)).toBeNull();
  });

  it("a message queued after the pick goes in the reply's chat, and the pick is dropped", async () => {
    const ends = (() => {
      const all: ((stop: string) => void)[] = [];
      api.sendChat.mockReset().mockImplementation(() => new Promise<string>((resolve) => all.push(resolve)));
      return all;
    })();
    const { rerender } = render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("first");
    emit(id, { kind: "done", stop: "end_turn" });
    rerender(<Chat onOpenSettings={() => {}} open={{ id: "shell-saved", n: 1 }} />);
    await userEvent.type(screen.getByLabelText("Message"), "then deploy{Enter}");
    await act(async () => ends[0]("end_turn"));
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(2));
    expect(api.sendChat.mock.calls[1].slice(0, 2)).toEqual([id, "then deploy"]);
    expect(api.loadChat).not.toHaveBeenCalled();
    expect(api.closeChat).not.toHaveBeenCalled();
    await act(async () => ends[1]("end_turn"));
  });

  it("a waiting pick is dropped by picking the open chat again, or by a refused mission", async () => {
    const finish = holdTurn();
    api.loadChat.mockResolvedValue(SAVED);
    const { rerender } = render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("test");
    emit(id, { kind: "done", stop: "end_turn" });
    rerender(<Chat onOpenSettings={() => {}} open={{ id: "shell-saved", n: 1 }} />);
    rerender(<Chat onOpenSettings={() => {}} open={{ id, n: 2 }} />);
    await act(async () => finish());
    expect(screen.getByText("test")).toBeVisible();
    expect(api.loadChat).not.toHaveBeenCalled();

    const finishTwo = holdTurn();
    await startTurn("again");
    emit(id, { kind: "done", stop: "end_turn" });
    rerender(<Chat onOpenSettings={() => {}} open={{ id: "shell-saved", n: 3 }} />);
    rerender(
      <Chat
        onOpenSettings={() => {}}
        open={{ id: "shell-saved", n: 3 }}
        prefill={{ text: "Mission", n: 1, conversationId: "shell-m1" }}
      />,
    );
    expect(screen.getByText(MISSION_BUSY)).toBeVisible();
    await act(async () => finishTwo());
    expect(screen.getByText("again")).toBeVisible();
    expect(api.loadChat).not.toHaveBeenCalled();
  });

  it("a waiting pick says the reply still runs when the wrap-up is slow", async () => {
    const finish = holdTurn();
    const { rerender } = render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("test");
    emit(id, { kind: "done", stop: "end_turn" });
    rerender(<Chat onOpenSettings={() => {}} open={{ id: "shell-saved", n: 1 }} />);
    expect(screen.queryByText(OPEN_BUSY)).toBeNull();
    expect(await screen.findByText(OPEN_BUSY, {}, { timeout: 3000 })).toBeVisible();
    api.loadChat.mockResolvedValue(SAVED);
    await act(async () => finish());
    expect(await screen.findByText("pick a paint colour")).toBeVisible();
    expect(screen.queryByText(OPEN_BUSY)).toBeNull();
  });

  it("a reply past its done still holds the chat while a queued message waits", async () => {
    const finish = holdTurn();
    const { rerender } = render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("first");
    await userEvent.type(screen.getByLabelText("Message"), "then deploy{Enter}");
    emit(id, { kind: "done", stop: "end_turn" });
    rerender(<Chat onOpenSettings={() => {}} open={{ id: "shell-saved", n: 1 }} />);
    expect(screen.getByText(OPEN_BUSY)).toBeVisible();
    expect(api.loadChat).not.toHaveBeenCalled();
    await act(async () => finish());
  });

  it("the still-running error goes once the reply ends", async () => {
    const finish = holdTurn();
    const { rerender } = render(<Chat onOpenSettings={() => {}} />);
    await startTurn("hello");
    rerender(<Chat onOpenSettings={() => {}} open={{ id: "shell-saved", n: 1 }} />);
    expect(screen.getByText(OPEN_BUSY)).toBeVisible();
    await act(async () => finish());
    await waitFor(() => expect(screen.queryByText(OPEN_BUSY)).toBeNull());
  });

  it("a reply that ends with no done event, or with an error, lets the next chat open", async () => {
    api.loadChat.mockResolvedValue(SAVED);
    const finish = holdTurn();
    const { rerender } = render(<Chat onOpenSettings={() => {}} />);
    await startTurn("hello");
    await act(async () => finish());
    rerender(<Chat onOpenSettings={() => {}} open={{ id: "shell-saved", n: 1 }} />);
    expect(await screen.findByText("pick a paint colour")).toBeVisible();
    expect(screen.queryByText(OPEN_BUSY)).toBeNull();

    api.sendChat.mockReset().mockRejectedValue("The model stopped answering.");
    await userEvent.type(screen.getByLabelText("Message"), "again{Enter}");
    expect(await screen.findByText(/The model stopped answering/)).toBeVisible();
    api.loadChat.mockResolvedValue({ ...SAVED, id: "shell-two", lines: [{ role: "user", text: "second chat" }] });
    rerender(<Chat onOpenSettings={() => {}} open={{ id: "shell-two", n: 2 }} />);
    expect(await screen.findByText("second chat")).toBeVisible();
    expect(screen.queryByText(OPEN_BUSY)).toBeNull();
  });

  it("deleting the open chat in History starts a new one; another chat's delete changes nothing", async () => {
    api.getStartChat.mockResolvedValue(SAVED);
    api.sendChat.mockResolvedValue("end_turn");
    const { rerender } = render(<Chat onOpenSettings={() => {}} />);
    await screen.findByText("pick a paint colour");
    rerender(<Chat onOpenSettings={() => {}} deleted={{ id: "shell-other", n: 1 }} />);
    expect(screen.getByText("pick a paint colour")).toBeVisible();
    rerender(<Chat onOpenSettings={() => {}} deleted={{ id: "shell-saved", n: 2 }} />);
    await waitFor(() => expect(screen.queryByText("pick a paint colour")).toBeNull());
    const id = await startTurn("new start");
    expect(id).not.toBe("shell-saved");
  });

  it("Clear chat deletes the open chat's saved history and starts a new one", async () => {
    api.getStartChat.mockResolvedValue(SAVED);
    api.clearChat.mockResolvedValue(null);
    api.sendChat.mockResolvedValue("end_turn");
    const { rerender } = render(<Chat onOpenSettings={() => {}} />);
    await screen.findByText("pick a paint colour");
    rerender(<Chat onOpenSettings={() => {}} clear={{ n: 1 }} />);
    await waitFor(() => expect(screen.queryByText("pick a paint colour")).toBeNull());
    expect(api.clearChat).toHaveBeenCalledWith("shell-saved");
    const id = await startTurn("new start");
    expect(id).not.toBe("shell-saved");
  });

  it("Clear chat on Main opens the empty Main the backend hands back", async () => {
    api.getStartChat.mockResolvedValue({ ...SAVED, id: "shell-main-1", title: "Main" });
    api.clearChat.mockReset().mockResolvedValue({ id: "shell-main-2", title: "Main", lines: [], earlier: 0 });
    api.sendChat.mockResolvedValue("end_turn");
    const { rerender } = render(<Chat onOpenSettings={() => {}} />);
    await screen.findByText("pick a paint colour");
    rerender(<Chat onOpenSettings={() => {}} clear={{ n: 1 }} />);
    await waitFor(() => expect(screen.queryByText("pick a paint colour")).toBeNull());
    expect(api.clearChat).toHaveBeenCalledWith("shell-main-1");
    expect(await startTurn("fresh start")).toBe("shell-main-2");
  });

  it("Clear chat keeps the chat on screen when the delete fails", async () => {
    api.getStartChat.mockResolvedValue(SAVED);
    api.clearChat.mockRejectedValue("disk is read-only");
    const { rerender } = render(<Chat onOpenSettings={() => {}} />);
    await screen.findByText("pick a paint colour");
    rerender(<Chat onOpenSettings={() => {}} clear={{ n: 1 }} />);
    expect(await screen.findByText(/disk is read-only/)).toBeVisible();
    expect(screen.getByText("pick a paint colour")).toBeVisible();
  });

  it("a bubble's ··· deletes that message after asking, and a reply goes with its tools", async () => {
    api.getStartChat.mockResolvedValue({
      ...SAVED,
      lines: [...SAVED.lines, { role: "user", text: "and the trim?" }, { role: "assistant", text: "White." }],
    });
    api.forgetMessage.mockReset().mockResolvedValue(true);
    render(<Chat onOpenSettings={() => {}} />);
    const ask = await screen.findByText("and the trim?");
    await userEvent.click(within(ask).getByRole("button", { name: MESSAGE_MENU_LABEL }));
    await userEvent.click(screen.getByRole("menuitem", { name: DELETE_MESSAGE_LABEL }));
    const dialog = screen.getByRole("alertdialog", { name: DELETE_ASK_QUESTION });
    await userEvent.click(within(dialog).getByRole("button", { name: "Keep" }));
    expect(api.forgetMessage).not.toHaveBeenCalled();
    expect(screen.getByText("and the trim?")).toBeVisible();

    await userEvent.click(within(ask).getByRole("button", { name: MESSAGE_MENU_LABEL }));
    await userEvent.click(screen.getByRole("menuitem", { name: DELETE_MESSAGE_LABEL }));
    await userEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(api.forgetMessage).toHaveBeenCalledWith("shell-saved", {
      from: 3,
      to: 4,
      reply: false,
      text: "and the trim?",
      later: 0,
    });
    await waitFor(() => expect(screen.queryByText("and the trim?")).toBeNull());

    // The ··· adds nothing to a bubble's text, so copying or reading it
    // gives the message alone.
    expect(screen.getByText("White.").closest(".bubble")?.textContent).toBe("White.");

    // Right-click on a reply: the whole reply to "pick a paint colour" goes,
    // its tools line included, now that "White." answers it too.
    fireEvent.contextMenu(screen.getByText("White."));
    await userEvent.click(screen.getByRole("menuitem", { name: DELETE_MESSAGE_LABEL }));
    expect(screen.getByRole("alertdialog", { name: DELETE_REPLY_QUESTION })).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(api.forgetMessage).toHaveBeenLastCalledWith(
      "shell-saved",
      expect.objectContaining({ reply: true, text: "pick a paint colour", later: 0 }),
    );
    await waitFor(() => expect(screen.queryByText("White.")).toBeNull());
    expect(screen.queryByText("Tools ran: read_file, web_search")).toBeNull();
    expect(screen.getByText("pick a paint colour")).toBeVisible();
  });

  it("a delete the saved chat refuses keeps the message and says why", async () => {
    api.getStartChat.mockResolvedValue(SAVED);
    api.forgetMessage.mockReset().mockRejectedValue("Wait for the reply to finish, then delete the message.");
    render(<Chat onOpenSettings={() => {}} />);
    fireEvent.contextMenu(await screen.findByText("pick a paint colour"));
    await userEvent.click(screen.getByRole("menuitem", { name: DELETE_MESSAGE_LABEL }));
    await userEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(await screen.findByText(/Wait for the reply to finish/)).toBeVisible();
    expect(screen.getByText("pick a paint colour")).toBeVisible();
  });

  it("the web app offers no message menu", async () => {
    api.getStartChat.mockResolvedValue(SAVED);
    api.sendChat.mockResolvedValue("end_turn");
    render(<Chat onOpenSettings={() => {}} remote />);
    await startTurn("hello from the phone");
    expect(screen.queryByRole("button", { name: MESSAGE_MENU_LABEL })).toBeNull();
  });

  it("deleting a message above two answered questions leaves each card its own answer", async () => {
    const finish = holdTurn();
    api.forgetMessage.mockReset().mockResolvedValue(true);
    api.answerElicit.mockReset().mockResolvedValue();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("plan it");
    emit(id, question("ask-a", "Which colour?", ["Red", "Blue"]));
    await userEvent.click(screen.getByRole("button", { name: "Red" }));
    emit(id, question("ask-b", "Which size?", ["Small", "Large"]));
    await userEvent.click(screen.getByRole("button", { name: "Large" }));
    finish();
    await waitFor(() => expect(screen.queryByRole("button", { name: "Stop" })).toBeNull());
    const cards = () => screen.getAllByRole("group", { name: "Loadout asks" }).map((card) => card.textContent);
    expect(cards()).toEqual(["Loadout asksWhich colour?You chose: Red", "Loadout asksWhich size?You chose: Large"]);

    const ask = screen.getByText("plan it");
    await userEvent.click(within(ask).getByRole("button", { name: MESSAGE_MENU_LABEL }));
    await userEvent.click(screen.getByRole("menuitem", { name: DELETE_MESSAGE_LABEL }));
    await userEvent.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(screen.queryByText("plan it")).toBeNull());
    expect(cards()).toEqual(["Loadout asksWhich colour?Sent.", "Loadout asksWhich size?Sent."]);
  });

  it("deleting a message above two open questions keeps what was typed out of the other card", async () => {
    api.sendChat.mockResolvedValue("end_turn");
    api.forgetMessage.mockReset().mockResolvedValue(true);
    api.answerElicit.mockReset().mockResolvedValue();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("go");
    await waitFor(() => expect(screen.queryByRole("button", { name: "Stop" })).toBeNull());
    act(() => api.handler?.({ conversationId: id, event: question("ask-a", "Token for A?", []), helper: true }));
    act(() => api.handler?.({ conversationId: id, event: question("ask-b", "Token for B?", []), helper: true }));
    const boxes = () => screen.getAllByRole<HTMLInputElement>("textbox", { name: "Answer" }).map((box) => box.value);
    await userEvent.type(screen.getAllByRole("textbox", { name: "Answer" })[0], "alpha-for-A");
    expect(boxes()).toEqual(["alpha-for-A", ""]);

    const ask = screen.getByText("go");
    await userEvent.click(within(ask).getByRole("button", { name: MESSAGE_MENU_LABEL }));
    await userEvent.click(screen.getByRole("menuitem", { name: DELETE_MESSAGE_LABEL }));
    await userEvent.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(screen.queryByText("go")).toBeNull());
    expect(boxes()).toEqual(["", ""]);

    const second = screen.getAllByRole("group", { name: "Loadout asks" })[1];
    expect(second).toHaveTextContent("Token for B?");
    expect(within(second).getByRole("button", { name: "Send" })).toBeDisabled();
    await userEvent.type(within(second).getByRole("textbox", { name: "Answer" }), "beta-for-B");
    await userEvent.click(within(second).getByRole("button", { name: "Send" }));
    expect(api.answerElicit.mock.calls).toEqual([[id, "ask-b", "accept", "answer", "beta-for-B"]]);
  });

  it("Show earlier loads more of a long chat's start", async () => {
    api.getStartChat.mockResolvedValue({ ...SAVED, earlier: 5 });
    api.loadChat.mockResolvedValue({
      ...SAVED,
      lines: [{ role: "user", text: "the very first ask" }, ...SAVED.lines],
      earlier: 0,
    });
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.click(await screen.findByRole("button", { name: "Show earlier messages (5)" }));
    expect(api.loadChat).toHaveBeenCalledWith("shell-saved", 3 + EARLIER_STEP);
    expect(await screen.findByText("the very first ask")).toBeVisible();
    expect(screen.queryByRole("button", { name: /Show earlier/ })).toBeNull();
  });
});

describe("Chat queue", () => {
  /** Each sendChat waits until its `end(stop)` is called. */
  function turns() {
    const ends: ((stop: string) => void)[] = [];
    api.sendChat.mockReset().mockImplementation(() => new Promise<string>((resolve) => ends.push(resolve)));
    return ends;
  }

  beforeEach(() => {
    api.getStartChat.mockReset().mockResolvedValue(null);
    api.quickChips.mockReset().mockResolvedValue([]);
    api.chipOutcome.mockReset().mockResolvedValue(true);
    api.getSettings.mockReset().mockResolvedValue(SETTINGS);
    api.cancelChat.mockReset().mockResolvedValue();
    api.steerChat.mockReset();
    api.sideChat.mockReset();
    api.tellMain.mockReset();
    api.readTurn.mockReset().mockImplementation((text: string) =>
      Promise.resolve({ route: "queue", text, ack: "Queued for after this reply" }),
    );
  });

  it("/deep-research typed while a reply runs waits its turn, then goes as typed", async () => {
    const ends = turns();
    api.runSlash.mockReset().mockResolvedValue({ kind: "queue", text: "/deep-research harbor tides" });
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "hello{Enter}");
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(1));
    await userEvent.type(screen.getByLabelText("Message"), "/deep-research harbor tides{Enter}");
    await waitFor(() => expect(api.runSlash).toHaveBeenCalledTimes(1));
    expect(api.runSlash.mock.calls[0][0]).toBe("/deep-research harbor tides");
    expect(api.sendChat).toHaveBeenCalledTimes(1);
    expect(screen.getByLabelText("Message")).toHaveValue("");
    await act(async () => ends[0]("end_turn"));
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(2));
    expect(api.sendChat.mock.calls[1][1]).toBe("/deep-research harbor tides");
    expect(api.runSlash).toHaveBeenCalledTimes(1);
  });

  it("a side question gets its own answer beside the reply, never in the chat", async () => {
    const ends = turns();
    let endSide: (stop: string) => void = () => {};
    api.sideChat.mockImplementation(() => new Promise<string>((resolve) => (endSide = resolve)));
    api.readTurn.mockResolvedValue({ route: "side", text: "what is a lockfile", ack: "Answering on the side" });
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "update the deps{Enter}");
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(1));
    const id = api.sendChat.mock.calls[0][0];
    await userEvent.type(screen.getByLabelText("Message"), "btw what is a lockfile{Enter}");
    await waitFor(() =>
      expect(api.sideChat).toHaveBeenCalledWith(id, `${id}~side-1`, "what is a lockfile", "update the deps", ""),
    );
    const card = screen.getByRole("group", { name: "On the side: what is a lockfile" });
    expect(card).toHaveTextContent("Answering…");
    emit(`${id}~side-1`, { kind: "text", text: "It pins exact versions." });
    expect(card).toHaveTextContent("It pins exact versions.");
    await act(async () => endSide("end_turn"));
    // The main chat never shows the side answer, and nothing queued.
    expect(api.sendChat).toHaveBeenCalledTimes(1);
    api.tellMain.mockResolvedValue({ text: "About lockfiles: It pins exact versions.", steered: true });
    await userEvent.click(screen.getByRole("button", { name: "Tell the main task" }));
    expect(api.tellMain).toHaveBeenCalledWith(id, "what is a lockfile", "It pins exact versions.");
    await waitFor(() => expect(screen.queryByRole("group", { name: /On the side/ })).toBeNull());
    await act(async () => ends[0]("end_turn"));
    expect(api.sendChat).toHaveBeenCalledTimes(1);
  });

  it("a correction during a reply steers it, and stop stops it", async () => {
    turns();
    api.steerChat.mockResolvedValue(true);
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "write the docs{Enter}");
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(1));
    const id = api.sendChat.mock.calls[0][0];
    api.readTurn.mockResolvedValueOnce({ route: "steer", text: "use British spelling", ack: "Added to the reply" });
    await userEvent.type(screen.getByLabelText("Message"), "use British spelling{Enter}");
    await waitFor(() => expect(api.steerChat).toHaveBeenCalledWith(id, "use British spelling"));
    api.readTurn.mockResolvedValueOnce({ route: "stop", text: "stop", ack: "Stopping" });
    await userEvent.type(screen.getByLabelText("Message"), "stop{Enter}");
    await waitFor(() => expect(api.cancelChat).toHaveBeenCalledWith(id));
  });

  it("lines up messages sent during a reply, reports them, and sends the next when it ends", async () => {
    const ends = turns();
    const onQueue = vi.fn();
    render(<Chat onOpenSettings={() => {}} onQueue={onQueue} />);
    await userEvent.type(screen.getByLabelText("Message"), "first{Enter}");
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(1));
    const box = screen.getByLabelText("Message");
    await userEvent.type(box, "then deploy");
    expect(screen.getByRole("button", { name: "Send" })).toBeVisible();
    await userEvent.type(box, "{Enter}");
    await userEvent.type(box, "and tell me{Enter}");
    expect(box).toHaveValue("");
    expect(api.sendChat).toHaveBeenCalledTimes(1);
    // Queued messages are not in the chat log until they send.
    expect(screen.queryByText("then deploy")).toBeNull();
    const id = api.sendChat.mock.calls[0][0];
    expect(onQueue).toHaveBeenLastCalledWith(id, ["then deploy", "and tell me"]);
    await act(async () => ends[0]("end_turn"));
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(2));
    expect(api.sendChat.mock.calls[1][1]).toBe("then deploy");
    expect(onQueue).toHaveBeenLastCalledWith(id, ["and tell me"]);
    await act(async () => ends[1]("end_turn"));
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(3));
    expect(api.sendChat.mock.calls[2][1]).toBe("and tell me");
    await act(async () => ends[2]("end_turn"));
    expect(onQueue).toHaveBeenLastCalledWith(id, []);
  });

  it("Clear chat mid-reply opens a chat that shows Send, and the old chat's waiting words go back in the box", async () => {
    const ends = turns();
    api.clearChat.mockResolvedValue(null);
    const { rerender } = render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "first{Enter}");
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(1));
    const old = api.sendChat.mock.calls[0][0];
    await userEvent.type(screen.getByLabelText("Message"), "then deploy{Enter}");
    rerender(<Chat onOpenSettings={() => {}} clear={{ n: 1 }} />);
    await waitFor(() => expect(api.clearChat).toHaveBeenCalledWith(old));
    await waitFor(() => expect(screen.queryByRole("button", { name: "Stop" })).toBeNull());
    expect(screen.getByLabelText("Message")).toHaveValue("then deploy");
    // The old reply ends after the new chat is open: nothing of it sends here.
    await act(async () => ends[0]("end_turn"));
    expect(api.sendChat).toHaveBeenCalledTimes(1);
    expect(screen.getByLabelText("Message")).toHaveValue("then deploy");
  });

  it("Clear chat on Main mid-reply puts the old Main's waiting words back in the box", async () => {
    const ends = turns();
    api.clearChat.mockResolvedValue({ id: "shell-main-2", title: "Main", lines: [], earlier: 0 });
    const { rerender } = render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "first{Enter}");
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(1));
    const old = api.sendChat.mock.calls[0][0];
    await userEvent.type(screen.getByLabelText("Message"), "then deploy{Enter}");
    rerender(<Chat onOpenSettings={() => {}} clear={{ n: 1 }} />);
    await waitFor(() => expect(api.clearChat).toHaveBeenCalledWith(old));
    await waitFor(() => expect(screen.getByLabelText("Message")).toHaveValue("then deploy"));
    await act(async () => ends[0]("end_turn"));
    expect(api.sendChat).toHaveBeenCalledTimes(1);
    expect(screen.getByLabelText("Message")).toHaveValue("then deploy");
  });

  it("a message typed in the new chat while the old reply ends goes to the new chat", async () => {
    const ends = turns();
    api.clearChat.mockResolvedValue(null);
    const { rerender } = render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "first{Enter}");
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(1));
    const old = api.sendChat.mock.calls[0][0];
    rerender(<Chat onOpenSettings={() => {}} clear={{ n: 1 }} />);
    await waitFor(() => expect(api.clearChat).toHaveBeenCalledWith(old));
    await userEvent.type(screen.getByLabelText("Message"), "fresh start");
    await userEvent.click(screen.getByRole("button", { name: "Send" }));
    expect(api.sendChat).toHaveBeenCalledTimes(1);
    await act(async () => ends[0]("cancelled"));
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(2));
    expect(api.sendChat.mock.calls[1][0]).not.toBe(old);
    expect(api.sendChat.mock.calls[1][1]).toBe("fresh start");
  });

  it("Stop puts what waited back in the box instead of sending it", async () => {
    const ends = turns();
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "first{Enter}");
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(1));
    await userEvent.type(screen.getByLabelText("Message"), "then deploy{Enter}");
    await userEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(api.cancelChat).toHaveBeenCalledTimes(1);
    expect(screen.getByLabelText("Message")).toHaveValue("then deploy");
    await act(async () => ends[0]("cancelled"));
    expect(api.sendChat).toHaveBeenCalledTimes(1);
  });

  it("Remove puts the words back in an empty box; Steer now hands them to the reply", async () => {
    const ends = turns();
    api.steerChat.mockResolvedValue(true);
    const onQueue = vi.fn();
    const chat = (queueAct?: QueueAct) => <Chat onOpenSettings={() => {}} onQueue={onQueue} queueAct={queueAct} />;
    const { rerender } = render(chat());
    await userEvent.type(screen.getByLabelText("Message"), "first{Enter}");
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(1));
    const id = api.sendChat.mock.calls[0][0];
    await userEvent.type(screen.getByLabelText("Message"), "then deploy{Enter}");
    await userEvent.type(screen.getByLabelText("Message"), "use staging{Enter}");
    rerender(chat({ op: "remove", index: 0, n: 1 }));
    await waitFor(() => expect(screen.getByLabelText("Message")).toHaveValue("then deploy"));
    // The queue is reported from an effect, which can run after the box shows the words.
    await waitFor(() => expect(onQueue).toHaveBeenLastCalledWith(id, ["use staging"]));
    rerender(chat({ op: "steer", index: 0, n: 2 }));
    await waitFor(() => expect(api.steerChat).toHaveBeenCalledWith(id, "use staging"));
    await waitFor(() => expect(onQueue).toHaveBeenLastCalledWith(id, []));
    // Steered words show as sent; nothing more goes when the reply ends.
    expect(screen.getByText("use staging")).toBeVisible();
    await act(async () => ends[0]("end_turn"));
    expect(api.sendChat).toHaveBeenCalledTimes(1);
  });

  it("a Halt puts the queue back in the box", async () => {
    const ends = turns();
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "first{Enter}");
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(1));
    await userEvent.type(screen.getByLabelText("Message"), "then deploy{Enter}");
    await act(async () => ends[0]("halted"));
    expect(screen.getByLabelText("Message")).toHaveValue("then deploy");
    expect(api.sendChat).toHaveBeenCalledTimes(1);
  });

  it("as the Code tab's agent window it starts on a new session, not the last chat", async () => {
    api.getStartChat.mockResolvedValue(SAVED);
    const onConversation = vi.fn();
    const onBusy = vi.fn();
    const view = render(<Chat onOpenSettings={() => {}} embedded onConversation={onConversation} onBusy={onBusy} />);
    await waitFor(() => expect(api.getSettings).toHaveBeenCalled());
    expect(api.getStartChat).not.toHaveBeenCalled();
    expect(screen.queryByText("pick a paint colour")).not.toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Agent" })).toHaveClass("chat", "embedded");
    expect(screen.getByText(AGENT_EMPTY)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "New session" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "New chat" })).not.toBeInTheDocument();
    expect(onConversation).toHaveBeenCalledTimes(1);
    const first = onConversation.mock.calls[0][0];
    expect(first).toMatch(/^shell-/);
    expect(onBusy).toHaveBeenLastCalledWith(false);

    // A reply reports busy while it runs; a message typed meanwhile waits in sight.
    const finish = holdTurn();
    await userEvent.type(screen.getByLabelText(AGENT_PLACEHOLDER), "add a health check{Enter}");
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledWith(first, "add a health check", undefined));
    expect(onBusy).toHaveBeenLastCalledWith(true);
    api.readTurn.mockResolvedValue({ route: "queue", text: "then deploy", ack: "" });
    await userEvent.type(screen.getByLabelText(AGENT_PLACEHOLDER), "then deploy{Enter}");
    expect(await screen.findByText(waitingLine(1))).toBeInTheDocument();
    expect(waitingLine(1)).toBe("1 message waits for this reply to end.");
    expect(waitingLine(3)).toBe("3 messages wait for this reply to end.");
    api.sendChat.mockResolvedValue("end_turn");
    await act(async () => finish());
    await waitFor(() => expect(onBusy).toHaveBeenLastCalledWith(false));

    // A typed command tells the tab, which reads the open project again.
    const onRan = vi.fn();
    api.runSlash.mockReset().mockResolvedValue({ kind: "notice", text: "Bound /work/dawn" });
    view.rerender(
      <Chat onOpenSettings={() => {}} embedded onConversation={onConversation} onBusy={onBusy} onRan={onRan} />,
    );
    await userEvent.type(screen.getByLabelText(AGENT_PLACEHOLDER), "/project bind /work/dawn{Enter}");
    expect(await screen.findByText("Bound /work/dawn")).toBeInTheDocument();
    expect(api.runSlash).toHaveBeenCalledWith("/project bind /work/dawn", first);
    expect(onRan).toHaveBeenCalledTimes(1);

    // New session tells the tab the session changed.
    await userEvent.click(screen.getByRole("button", { name: "New session" }));
    await waitFor(() => expect(onConversation).toHaveBeenCalledTimes(2));
    expect(onConversation.mock.calls[1][0]).not.toBe(first);
  });

  it("in the agent window a queued message is sent with no mode when the reply ends", async () => {
    const ends = turns();
    render(<Chat onOpenSettings={() => {}} embedded />);
    await userEvent.type(screen.getByLabelText(AGENT_PLACEHOLDER), "add a health check{Enter}");
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(1));
    const id = api.sendChat.mock.calls[0][0];
    await userEvent.type(screen.getByLabelText(AGENT_PLACEHOLDER), "then deploy{Enter}");
    expect(await screen.findByText("1 message waits for this reply to end.")).toBeInTheDocument();
    await act(async () => ends[0]("end_turn"));
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(2));
    expect(api.sendChat.mock.calls).toEqual([
      [id, "add a health check", undefined],
      [id, "then deploy", undefined],
    ]);
  });

  it("an embedded plan card sends the edited plan, or sends it back for changes", async () => {
    turns();
    api.answerElicit.mockReset().mockResolvedValue();
    render(<Chat onOpenSettings={() => {}} embedded lead />);
    await userEvent.type(screen.getByLabelText(AGENT_PLACEHOLDER), "tidy the build{Enter}");
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(1));
    const id = api.sendChat.mock.calls[0][0];
    const plan = (cardId: string): ChatEvent => ({
      kind: "elicit",
      id: cardId,
      server: "Plan",
      message: "1. Read build.rs",
      mode: "form",
      url: "",
      fieldName: null,
      fieldTitle: "",
      secret: false,
    });
    emit(id, plan("plan-1"));
    const first = screen.getByRole("group", { name: "The plan" });
    await userEvent.type(within(first).getByRole("textbox", { name: "What should change" }), "add a test step");
    await userEvent.click(within(first).getByRole("button", { name: "Ask for changes" }));
    expect(api.answerElicit).toHaveBeenLastCalledWith(
      id,
      "plan-1",
      "accept",
      "reply",
      "Change the plan: add a test step",
    );
    expect(first).toHaveTextContent("Sent back for changes.");

    emit(id, plan("plan-2"));
    const second = screen.getAllByRole("group", { name: "The plan" })[1];
    const box = within(second).getByRole("textbox", { name: "Plan" });
    expect(box).toHaveValue("1. Read build.rs");
    await userEvent.type(box, "{Enter}2. Run the tests");
    await userEvent.click(within(second).getByRole("button", { name: "Approve plan" }));
    expect(api.answerElicit).toHaveBeenLastCalledWith(
      id,
      "plan-2",
      "accept",
      "plan",
      "1. Read build.rs\n2. Run the tests",
    );
    expect(second).toHaveTextContent("2. Run the testsApproved. Loadout can make the changes now.");
    expect(api.answerElicit).toHaveBeenCalledTimes(2);
  });

  it("a session in the agent window that has no lead keeps the plain plan card", async () => {
    turns();
    api.answerElicit.mockReset().mockResolvedValue();
    render(<Chat onOpenSettings={() => {}} embedded />);
    await userEvent.type(screen.getByLabelText(AGENT_PLACEHOLDER), "tidy the build{Enter}");
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(1));
    const id = api.sendChat.mock.calls[0][0];
    emit(id, {
      kind: "elicit",
      id: "plan-1",
      server: "Plan",
      message: "1. Read build.rs",
      mode: "form",
      url: "",
      fieldName: null,
      fieldTitle: "",
      secret: false,
    });
    const card = screen.getByRole("group", { name: "The plan" });
    expect(within(card).queryByRole("textbox")).toBeNull();
    await userEvent.click(within(card).getByRole("button", { name: "Approve plan" }));
    expect(api.answerElicit).toHaveBeenLastCalledWith(id, "plan-1", "accept", null, "");
  });

  it("view-plan in the agent window shows the saved plan", async () => {
    api.codeSessionPlan.mockReset().mockResolvedValueOnce("1. Edited").mockResolvedValueOnce(null);
    api.runSlash.mockReset().mockResolvedValue({ kind: "viewPlan" });
    const onConversation = vi.fn();
    render(<Chat onOpenSettings={() => {}} embedded onConversation={onConversation} />);
    await waitFor(() => expect(onConversation).toHaveBeenCalledTimes(1));
    const id = onConversation.mock.calls[0][0];
    await userEvent.type(screen.getByLabelText(AGENT_PLACEHOLDER), "/view-plan{Enter}");
    expect(await screen.findByRole("status", { name: "/view-plan result" })).toHaveTextContent("1. Edited");
    expect(api.codeSessionPlan).toHaveBeenLastCalledWith(id);
    // No saved plan and no plan card: it says there is none.
    await userEvent.type(screen.getByLabelText(AGENT_PLACEHOLDER), "/view-plan{Enter}");
    await waitFor(() => expect(screen.getAllByRole("status", { name: "/view-plan result" })).toHaveLength(2));
    expect(screen.getAllByRole("status", { name: "/view-plan result" })[1]).toHaveTextContent(
      "No plan yet. Use Plan mode.",
    );
    expect(NO_PLAN).toBe("No plan yet. Use Plan mode.");
    expect(api.codeSessionPlan).toHaveBeenCalledTimes(2);
  });

  it("view-plan in the Chat tab never reads a saved plan", async () => {
    api.codeSessionPlan.mockReset().mockResolvedValue("1. Edited");
    api.runSlash.mockReset().mockResolvedValue({ kind: "viewPlan" });
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "/view-plan{Enter}");
    expect(await screen.findByRole("status", { name: "/view-plan result" })).toHaveTextContent(
      "No plan yet. Use Plan mode.",
    );
    expect(api.codeSessionPlan).not.toHaveBeenCalled();
  });
});

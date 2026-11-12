// Typed wrappers over the Tauri commands in src-tauri/src/lib.rs. In the web
// app the same commands run on the paired desktop (./transport).
import type { UnlistenFn } from "@tauri-apps/api/event";
import { call as invoke, GAP_EVENT, inShell, subscribe } from "./transport";

/** How chat handles tools that change things (the cabin's permission mode). */
export type Approvals = "ask" | "auto" | "always";

export interface SignedIn {
  name: string | null;
  email: string | null;
}

export interface Settings {
  closeToTray: boolean;
  hasApiKey: boolean;
  /** Signed in with Grok: who, when xAI said. */
  signedIn?: SignedIn | null;
  /** The folder picked in Settings, empty for the default. */
  workspace: string;
  /** The folder the agent actually works in. */
  workspacePath: string;
  approvals: Approvals;
  /** Settings → Let the app control the desktop. */
  desktopControl: boolean;
  /** Settings → Appearance. */
  theme: Theme;
  /** Settings → About. */
  version: string;
  /** Settings → General → the summon hotkey. */
  summonHotkey: boolean;
  /** Settings → General → notifications while the window is in the background. */
  notifications: boolean;
}

/** What this platform runs (src-tauri/src/capabilities.rs). */
export interface Capabilities {
  desktopControl: boolean;
  pulse: boolean;
  localAi: boolean;
  tray: boolean;
  /** Settings → Web access: this computer serves the web app. */
  webAccess: boolean;
  /** Settings → Labs → Plugins. */
  plugins: boolean;
  /** Settings → Account → Sign in with Grok (desktop and iOS). */
  grokSignIn?: boolean;
  /** Settings → General → start at login. */
  autostart?: boolean;
  /** Settings → General → the summon hotkey; off on Wayland. */
  summonHotkey?: boolean;
  /** Settings → General → notifications while the window is in the background. */
  notifications?: boolean;
  /** The composer's push-to-talk mic (desktops and phones). */
  voice?: boolean;
  /** The Library page: this computer's Steam games. */
  library?: boolean;
  /** The web app, a remote for the desktop: settings and the key stay there. */
  remote?: boolean;
  /** Auto setup: the first-run window, Settings → Auto setup, text size. */
  autoSetup?: boolean;
  /** The Code tab: this computer's repos (desktops). */
  code?: boolean;
  /** The Mods page: this computer's moddable games (desktops). */
  mods?: boolean;
}

/** Until the report loads, or if it fails, only the core features show. */
export const CORE_ONLY: Capabilities = {
  desktopControl: false,
  pulse: false,
  localAi: false,
  tray: false,
  webAccess: false,
  plugins: false,
  library: false,
};

/** The web app gets chat, memory, automations, connectors and Pulse. */
export const WEB: Capabilities = { ...CORE_ONLY, pulse: true, remote: true };

export function getCapabilities(): Promise<Capabilities> {
  if (!inShell()) return Promise.resolve(WEB);
  return invoke<Capabilities>("get_capabilities");
}

export type Answer = "allow" | "always" | "deny";

/** One engine event for a conversation, from `chat://event`. */
export type ChatEvent =
  | { kind: "text"; text: string }
  | { kind: "tool"; id: string; title: string; status: string; detail: string }
  | {
      kind: "permission";
      id: string;
      title: string;
      action: string;
      reason: string;
    }
  /** A permission card answered away from the page, such as Deny on its notification. */
  | { kind: "answered"; id: string; answer: Answer }
  | { kind: "error"; message: string }
  | { kind: "done"; stop: string }
  | { kind: "plan"; text: string }
  /** One-tap follow-ups for the reply that just ended; most replies get none. */
  | { kind: "suggestions"; items: QuickChip[] }
  /** A game goal the reply noticed, to Track or leave with Not now. */
  | { kind: "goal"; id: string; title: string }
  | {
      kind: "elicit";
      id: string;
      /** "Plan" for a plan to approve, "Question" for the agent's question, or the connector. */
      server: string;
      message: string;
      mode: "form" | "url";
      url: string;
      fieldName: string | null;
      fieldTitle: string;
      secret: boolean;
      /** Answers to tap for the agent's question, the recommended one first. */
      options?: ElicitChoice[];
    };

/** A quick chip: the cabin's engine picked it for this moment (`chips.rs`).
 * Ask puts `value` in the box, or sends it when the box is empty; Go opens
 * `view`. Only these two cross from the shell. */
export interface QuickChip {
  id: string;
  label: string;
  value: string;
  action: "ask" | "go";
  /** Why it is here, for the chip's tip. */
  hint: string;
  primary: boolean;
  view?: string;
  /** A follow-up's reply mode, put back on tap. */
  mode?: SessionMode;
}

/** The composer's row, Home's row and the empty palette's, as the log names them. */
export type ChipSurface = "composer" | "home" | "palette";

/** The row for the open chat and its draft, or Home's. Empty with Settings →
 * Behavior → Suggestions off. */
export function quickChips(
  surface: ChipSurface,
  conversationId?: string,
  draft?: string,
  mode?: SessionMode,
): Promise<QuickChip[]> {
  return invoke<QuickChip[]>("quick_chips", {
    surface,
    conversationId,
    draft,
    mode,
    clock: wallClock(),
  });
}

/** A tap (`picked`) or a hide on a chip of that row; it teaches the ranker.
 * The page acts on the tap itself. */
export function chipOutcome(
  surface: ChipSurface,
  id: string,
  picked: boolean,
): Promise<boolean> {
  return invoke<boolean>("chip_outcome", {
    surface,
    id,
    picked,
    clock: wallClock(),
  });
}

/** One answer on a question card; the label is what gets sent. */
export type ElicitChoice = {
  label: string;
  description: string;
  recommended: boolean;
};

/** How one message runs: Chat, Plan (read-only until the plan is approved)
 * or btw (a look-only side answer). The app picks it; `/plan` and `/chat`
 * name it for the next message. */
export type SessionMode = "chat" | "plan" | "btw";

export type ElicitAction = "accept" | "decline" | "cancel";

export interface ChatEnvelope {
  conversationId: string;
  event: ChatEvent;
  /** From a helper that can outlive the reply: shown even when no reply runs. */
  helper?: boolean;
}

export const CHAT_EVENT = "chat://event";

export function getSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings");
}

export type Theme =
  "system" | "dark" | "ops-dark" | "oled-black" | "soft-charcoal" | "light";

export function setTheme(theme: Theme): Promise<Settings> {
  return invoke<Settings>("set_theme", { theme });
}

export function saveSettings(
  closeToTray: boolean,
  workspace: string,
  approvals: Approvals,
  desktopControl: boolean,
): Promise<Settings> {
  return invoke<Settings>("save_settings", {
    closeToTray,
    workspace,
    approvals,
    desktopControl,
  });
}

/** Whether the OS starts the app at login. */
export function getAutostart(): Promise<boolean> {
  return invoke<boolean>("get_autostart");
}

/** Resolves with what the OS has after the change. */
export function setAutostart(on: boolean): Promise<boolean> {
  return invoke<boolean>("set_autostart", { on });
}

export function setSummonHotkey(on: boolean): Promise<Settings> {
  return invoke<Settings>("set_summon_hotkey", { on });
}

export function setNotifications(on: boolean): Promise<Settings> {
  return invoke<Settings>("set_notifications", { on });
}

export function setApiKey(key: string): Promise<Settings> {
  return invoke<Settings>("set_api_key", { key });
}

export function clearApiKey(): Promise<Settings> {
  return invoke<Settings>("clear_api_key");
}

/** Runs one prompt through the agent loop; resolves with the stop reason.
 * `attachments`: paths from `stageAttachment`, in the chips' order. */
/** Sends one message. With no `mode` the app reads the message and picks:
 * a big multi-step job plans first, anything else is answered. */
export function sendChat(
  conversationId: string,
  text: string,
  mode?: SessionMode,
  attachments: string[] = [],
): Promise<string> {
  return invoke<string>("send_chat", {
    conversationId,
    text,
    mode,
    attachments,
    clock: wallClock(),
  });
}

/** What a message typed during a reply is for (src-tauri/src/turns.rs). */
export type RunningRoute = "side" | "steer" | "queue" | "stop";

export interface TurnRead {
  route: RunningRoute;
  /** The words to act on, with a typed btw lead taken off. */
  text: string;
  /** The line shown at once, so the message never looks dropped. */
  ack: string;
}

/** Reads a message typed while a reply runs. Instant: no model call. */
export function readTurn(
  text: string,
  conversationId?: string,
): Promise<TurnRead> {
  return invoke<TurnRead>("read_turn", { text, conversationId });
}

/** Answers `question` beside the running reply, read-only, under `sideId`.
 * Resolves with the stop reason when the answer is in. */
export function sideChat(
  conversationId: string,
  sideId: string,
  question: string,
  ask: string,
  soFar: string,
): Promise<string> {
  return invoke<string>("side_chat", {
    conversationId,
    sideId,
    question,
    ask,
    soFar,
  });
}

/** Tell the main task: `steered` when the words joined the running reply;
 * otherwise send `text` as the next message. */
export interface Told {
  text: string;
  steered: boolean;
}

export function tellMain(
  conversationId: string,
  question: string,
  answer: string,
): Promise<Told> {
  return invoke<Told>("tell_main", { conversationId, question, answer });
}

/** A file waiting on the composer (src-tauri/src/attach.rs). */
export interface Staged {
  /** The copy in the chat's folder; send it back with the message. */
  path: string;
  name: string;
  kind: "image" | "text" | "document";
  /** "Picture shot.png", "File notes.md". */
  label: string;
  /** The note when the chat's model can't read it; it won't send. */
  status: string | null;
}

/** One picked or dropped file: staged, or why not. */
export interface StageOutcome {
  staged: Staged | null;
  error: string | null;
}

/** A pasted or dropped file's bytes, as base64. */
export function stageAttachment(
  conversationId: string,
  name: string,
  data: string,
): Promise<Staged> {
  return invoke<Staged>("stage_attachment", { conversationId, name, data });
}

/** Files dropped on the window, by path (the desktop window only). */
export function stageAttachmentPaths(
  conversationId: string,
  paths: string[],
): Promise<StageOutcome[]> {
  return invoke<StageOutcome[]>("stage_attachment_paths", {
    conversationId,
    paths,
  });
}

/** The + button: the system file picker, then each pick is staged. */
export function pickAttachments(
  conversationId: string,
): Promise<StageOutcome[]> {
  return invoke<StageOutcome[]>("pick_attachments", { conversationId });
}

export function removeAttachment(
  conversationId: string,
  path: string,
): Promise<void> {
  return invoke<void>("remove_attachment", { conversationId, path });
}

/** Files dropped on the desktop window arrive as paths. Outside the shell
 * this never fires; the page's own drop event has the files. */
export async function onFileDrop(
  handler: (paths: string[]) => void,
): Promise<UnlistenFn> {
  if (!inShell()) return () => {};
  const { getCurrentWebview } = await import("@tauri-apps/api/webview");
  return getCurrentWebview().onDragDropEvent((event) => {
    if (event.payload.type === "drop" && event.payload.paths.length > 0)
      handler(event.payload.paths);
  });
}

/** Push to talk: one WAV (base64) to text for the draft. Never sends. */
export function transcribe(wav: string): Promise<string> {
  return invoke<string>("transcribe", { wav });
}

/** Answers a plan, a question or a connector form; `value` goes under `field`. */
export function answerElicit(
  conversationId: string,
  id: string,
  action: ElicitAction,
  field: string | null = null,
  value = "",
): Promise<void> {
  return invoke<void>("answer_elicit", {
    conversationId,
    id,
    action,
    field,
    value,
  });
}

export function answerPermission(
  conversationId: string,
  callId: string,
  answer: Answer,
): Promise<void> {
  return invoke<void>("answer_permission", { conversationId, callId, answer });
}

/** Helper questions this chat still waits on, for when it opens. */
export function helperQuestions(conversationId: string): Promise<ChatEvent[]> {
  if (!inShell()) return Promise.resolve([]);
  return invoke<ChatEvent[]>("helper_questions", { conversationId });
}

export function cancelChat(conversationId: string): Promise<void> {
  return invoke<void>("cancel_chat", { conversationId });
}

/** Steer now: queued words join the running reply at its next step. False
 * when that chat has no reply running, so the words stay queued. */
export function steerChat(
  conversationId: string,
  text: string,
): Promise<boolean> {
  return invoke<boolean>("steer_chat", { conversationId, text });
}

export function closeChat(conversationId: string): Promise<void> {
  return invoke<void>("close_chat", { conversationId });
}

/** A saved chat on the History page (src-tauri/src/history.rs). */
export interface ChatRow {
  id: string;
  title: string;
  /** Unix ms the chat started. */
  created: number;
  /** Unix ms of its last saved turn. */
  updated: number;
  /** Pinned with /pin or the row's menu: it lists first. */
  pinned?: boolean;
  /** The id of the project whose folder the chat works in. */
  project?: string | null;
}

/** A project or folder from the sidebar list the cabin shares (projects.json). */
export interface ProjectNode {
  id: string;
  name: string;
  kind: "project" | "folder";
  parent?: string | null;
  open: boolean;
}

/** A Grok CLI session or a cabin chat: listed, never written. */
export interface ReadOnlyRow {
  id: string;
  title: string;
  /** Unix ms; 0 when unknown. */
  updated: number;
  source: "cli" | "cabin";
}

/** A tool step a History search matched. */
export interface StepRow {
  chat: string;
  turn: number;
  line: string;
}

/** Where Delete all stands: armed asks for a second click, deleted is done. */
export interface DeleteAllResult {
  state: "armed" | "deleted";
  count: number;
}

/** "tools": a run of tool steps, named on one muted line (not replayed as cards). */
export type LineRole = "user" | "assistant" | "tools";

export interface TranscriptLine {
  role: LineRole;
  text: string;
}

/** A saved chat as Chat shows it: the newest lines, and how many older ones were left out. */
export interface Transcript {
  id: string;
  title: string;
  lines: TranscriptLine[];
  earlier: number;
}

/** History is this computer's; the web app never calls these. */
export function listChats(): Promise<ChatRow[]> {
  return invoke<ChatRow[]>("list_chats");
}

export function searchChats(query: string): Promise<ChatRow[]> {
  return invoke<ChatRow[]>("search_chats", { query });
}

/** Opens a saved chat; the next message in it carries on from its saved turns. */
export function loadChat(id: string, limit?: number): Promise<Transcript> {
  return invoke<Transcript>("load_chat", { id, limit: limit ?? null });
}

/** The chat open when the app last closed, if it is still saved. */
export function getLastChat(): Promise<Transcript | null> {
  return invoke<Transcript | null>("get_last_chat");
}

export function renameChat(id: string, title: string): Promise<void> {
  return invoke<void>("rename_chat", { id, title });
}

/** Pins the chat, or unpins it; true when pinned now. */
export function pinChat(id: string): Promise<boolean> {
  return invoke<boolean>("pin_chat", { id });
}

/** The sidebar's projects and folders, for the History tree. */
export function listProjectNodes(): Promise<ProjectNode[]> {
  return invoke<ProjectNode[]>("list_project_nodes");
}

export function listReadOnlyChats(query: string): Promise<ReadOnlyRow[]> {
  return invoke<ReadOnlyRow[]>("list_read_only_chats", { query });
}

/** A read-only chat's newest lines; it is never resumed. */
export function viewReadOnlyChat(id: string): Promise<Transcript> {
  return invoke<Transcript>("view_read_only_chat", { id });
}

export function searchSteps(query: string): Promise<StepRow[]> {
  return invoke<StepRow[]>("search_steps", { query });
}

/** Delete all: the first call arms it; a second inside 5 s (and not within
 * half a second of the first) deletes every chat of this app. */
export function deleteAllChats(): Promise<DeleteAllResult> {
  return invoke<DeleteAllResult>("delete_all_chats");
}

export function cancelDeleteAll(): Promise<void> {
  return invoke<void>("cancel_delete_all");
}

/** Deletes the saved chat; a reply still running in it stops. */
export function deleteChat(id: string): Promise<void> {
  return invoke<void>("delete_chat", { id });
}

/** One row on the Hangout chat tab strip (or an Archived / Recently deleted menu). */
export interface ChatTab {
  id: string;
  title: string;
  pinned: boolean;
  updated: number;
  archivedAt?: number;
  deletedAt?: number;
}

export type HangoutMode = "talk" | "type";

/** Live tabs under the title bar (not archived, not soft-deleted). */
export function listChatTabs(): Promise<ChatTab[]> {
  return invoke<ChatTab[]>("list_chat_tabs");
}

export function listArchivedChats(): Promise<ChatTab[]> {
  return invoke<ChatTab[]>("list_archived_chats");
}

export function listDeletedChats(): Promise<ChatTab[]> {
  return invoke<ChatTab[]>("list_deleted_chats");
}

export function chatTabSwitch(id: string): Promise<ChatTab> {
  return invoke<ChatTab>("chat_tab_switch", { id });
}

export function chatTabPin(id: string): Promise<boolean> {
  return invoke<boolean>("chat_tab_pin", { id });
}

export function chatTabRename(id: string, title: string): Promise<void> {
  return invoke<void>("chat_tab_rename", { id, title });
}

export function chatTabArchive(id: string): Promise<ChatTab> {
  return invoke<ChatTab>("chat_tab_archive", { id });
}

export function chatTabUnarchive(id: string): Promise<ChatTab> {
  return invoke<ChatTab>("chat_tab_unarchive", { id });
}

export function chatTabSoftDelete(id: string): Promise<ChatTab> {
  return invoke<ChatTab>("chat_tab_soft_delete", { id });
}

export function chatTabRestoreDeleted(id: string): Promise<ChatTab> {
  return invoke<ChatTab>("chat_tab_restore_deleted", { id });
}

export function getHangoutMode(): Promise<HangoutMode> {
  return invoke<HangoutMode>("get_hangout_mode");
}

export function setHangoutMode(mode: HangoutMode): Promise<HangoutMode> {
  return invoke<HangoutMode>("set_hangout_mode", { mode });
}

/** A chat export on disk (src-tauri/src/export.rs). Open and Show in folder
 * take its token, never a path. */
export interface SavedFile {
  token: string;
  /** "trip-plan-20261106-1415.md" */
  name: string;
  /** "Chats" */
  folder: string;
  /** The whole path, for Copy path. */
  path: string;
  /** "Saved trip-plan-20261106-1415.md to Chats" */
  line: string;
  /** "Show in File Explorer", "Show in Finder" or "Show in folder". */
  revealLabel: string;
}

/** `/export` and History's Export: the chat as Markdown in your Chats folder. */
export function exportChat(
  id: string,
  format?: "html" | "json",
): Promise<SavedFile> {
  return invoke<SavedFile>("export_chat", { id, clock: wallClock(), format });
}

export function openSaved(token: string): Promise<void> {
  return invoke<void>("open_saved", { token });
}

export function revealSaved(token: string): Promise<void> {
  return invoke<void>("reveal_saved", { token });
}

/** Takes one message out of a saved chat after the page asked: the ask
 * `text`, or with `reply` the reply to it (no `text`: the reply before any
 * ask). False when the saved chat had no copy. A running reply refuses. */
export function forgetMessage(
  id: string,
  cut: { reply: boolean; text: string | null; later: number },
): Promise<boolean> {
  return invoke<boolean>("forget_message", {
    id,
    reply: cut.reply,
    text: cut.text,
    later: cut.later,
  });
}

/** Opens a reply's http(s) link in the default browser (`open_url`, which
 * refuses anything else). Shell only: in the web app links open in a new tab
 * and never reach the computer. */
export function openUrl(url: string): Promise<void> {
  if (!inShell())
    return Promise.reject(new Error("Links open in this browser's own tabs."));
  return invoke<void>("open_url", { url });
}

export function onChatEvent(
  handler: (envelope: ChatEnvelope) => void,
): Promise<UnlistenFn> {
  return subscribe<ChatEnvelope>(CHAT_EVENT, handler);
}

/** Sent after every Halt (top bar, Ctrl+Alt+H, the tray or Super+Shift+Esc). */
export const HALT_EVENT = "halt://done";

/** Halt: stops desktop control and every running prompt at once. Resolves
 * with the chats it stopped mid-reply. Desktop only; the web app and phones
 * are refused. */
export function haltDesktop(): Promise<string[]> {
  if (!inShell())
    return Promise.reject(new Error("Halt runs on the computer itself."));
  return invoke<string[]>("halt_desktop");
}

/** The chats a Halt stopped, whichever way it was pressed. */
export function onHalt(
  handler: (stopped: string[]) => void,
): Promise<UnlistenFn> {
  return subscribe<string[]>(HALT_EVENT, handler);
}

export interface HaltKeys {
  /** Super+Shift+Esc works from any app, not only this window. */
  global: boolean;
}

export function getHaltKeys(): Promise<HaltKeys> {
  return invoke<HaltKeys>("get_halt_keys");
}

/** The web app's stream was down longer than the desktop keeps events for. */
export function onEventGap(handler: () => void): Promise<UnlistenFn> {
  return subscribe<null>(GAP_EVENT, () => handler());
}

export interface AutomationRow {
  id: string;
  name: string;
  instructions: string;
  enabled: boolean;
  summary: string;
  health: string | null;
  /** The words for the Edit box; null when words can't say the schedule. */
  describe: string | null;
  /** It has a check command, so only the cabin runs it. */
  cabinOnly: boolean;
}

export interface RunResult {
  text: string;
  error: string;
  stop: string;
}

/** The local wall clock, so next-run times read in the user's own zone. */
export function wallClock(now: Date = new Date()) {
  return {
    weekday: now.getDay(),
    hour: now.getHours(),
    minute: now.getMinutes(),
  };
}

export function listAutomations(): Promise<AutomationRow[]> {
  return invoke<AutomationRow[]>("list_automations", { clock: wallClock() });
}

export function addAutomation(text: string): Promise<AutomationRow[]> {
  return invoke<AutomationRow[]>("add_automation", {
    text,
    clock: wallClock(),
  });
}

export function setAutomationEnabled(
  id: string,
  enabled: boolean,
): Promise<AutomationRow[]> {
  return invoke<AutomationRow[]>("set_automation_enabled", {
    id,
    enabled,
    clock: wallClock(),
  });
}

export function removeAutomation(id: string): Promise<AutomationRow[]> {
  return invoke<AutomationRow[]>("remove_automation", {
    id,
    clock: wallClock(),
  });
}

export function runAutomation(id: string): Promise<RunResult> {
  return invoke<RunResult>("run_automation", { id });
}

export function editAutomation(
  id: string,
  text: string,
): Promise<AutomationRow[]> {
  return invoke<AutomationRow[]>("edit_automation", {
    id,
    text,
    clock: wallClock(),
  });
}

/** A scheduled run the shell fired, from `automations://ran`. */
export interface ScheduledRun {
  id: string;
  name: string;
  result: RunResult;
}

export const AUTOMATION_RAN_EVENT = "automations://ran";

export function onAutomationRan(
  handler: (run: ScheduledRun) => void,
): Promise<UnlistenFn> {
  return subscribe<ScheduledRun>(AUTOMATION_RAN_EVENT, handler);
}

/** Tells the shell's scheduler the local time, so jobs fire at local times.
 * Only the desktop window sends it; a paired browser may be elsewhere. */
export function shareSchedulerClock(): Promise<void> {
  if (!inShell()) return Promise.resolve();
  return invoke<void>("set_scheduler_clock", { clock: wallClock() });
}

export interface MemoryRow {
  id: string;
  text: string;
  /** Why this line is here, including the chat turn when it has one. */
  source: string;
  /** When the line was written or last changed. */
  updated: string;
  proposed: boolean;
  /** Kept, but chat, the profile block and the nightly review skip it. */
  paused: boolean;
  /** The Personal group it shows under, by its plain name. */
  group: string;
  /** Claims this line corrects, joined for the panel. */
  corrects: string;
}

export interface MemoryNotes {
  global: string;
  workspace: string;
  globalPath: string;
  workspacePath: string;
  /** The cabin's AMR store is on, so recall does not read these files. */
  amr: boolean;
  /** Learned-memory writes. Off stops notes, proposals and edits. */
  writesOn: boolean;
  /** User-model rows, proposed ones included. */
  rows: MemoryRow[];
}

export interface MemoryHit {
  scope: string;
  body: string;
}

export function getMemory(): Promise<MemoryNotes> {
  return invoke<MemoryNotes>("get_memory");
}

/** `base` is the text the editor started from; a file that changed since is not overwritten. */
export function saveMemory(
  global: boolean,
  text: string,
  base: string,
): Promise<{ notes: MemoryNotes; redacted: boolean }> {
  return invoke("save_memory", { global, text, base });
}

/** Sign in with Grok, step one: the code to type at xAI's page. */
export interface GrokSignInStart {
  userCode: string;
  verificationUri: string;
  verificationUriComplete: string | null;
  interval: number;
  expiresIn: number;
}

export type GrokSignInPoll =
  | { status: "pending"; interval: number }
  | { status: "ready"; signedIn: SignedIn }
  | { status: "failed"; error: string }
  /** No answer came back this time; the shell still holds the code. */
  | { status: "noAnswer"; error: string };

export function startGrokSignIn(): Promise<GrokSignInStart> {
  return invoke<GrokSignInStart>("start_grok_sign_in");
}

export function pollGrokSignIn(): Promise<GrokSignInPoll> {
  return invoke<GrokSignInPoll>("poll_grok_sign_in");
}

export function grokSignOut(): Promise<Settings> {
  return invoke<Settings>("grok_sign_out");
}

/** A row of the composer's command list. `disabled` says why it can't run. */
export interface SlashHit {
  cmd: string;
  hint: string;
  insert: string;
  runOnPick: boolean;
  disabled: string | null;
}

/** What a slash line asked the chat to do. Slash lines never reach the model. */
export type SlashEffect =
  | { kind: "newChat" }
  | { kind: "mode"; mode: SessionMode; text: string | null }
  | {
      kind: "open";
      view:
        | "settings"
        | "memory"
        | "automations"
        | "connectors"
        | "history"
        | "skills"
        | "workboard"
        | "devices"
        | "updates";
    }
  | { kind: "notice"; text: string }
  | { kind: "import" }
  | { kind: "export" }
  | { kind: "exportAs"; format: "html" | "json" }
  | { kind: "retry" }
  | { kind: "undo" }
  | { kind: "stop" }
  | { kind: "compact" }
  /** A destructive command waits for a yes: run `yes` when the person agrees. */
  | { kind: "confirm"; prompt: string; yes: string }
  | { kind: "deleted"; id: string; title: string }
  /** `/queue`: held until the live reply ends, or sent at once. */
  | { kind: "queue"; text: string }
  /** `/palette`. */
  | { kind: "palette" }
  /** `/view-plan`: the latest plan from Plan mode. */
  | { kind: "viewPlan" }
  /** `/loop <words>`: saved as an automation, then its page opens. */
  | { kind: "loop"; seed: string }
  /** A page opens, with a line saying why. */
  | { kind: "openNote"; view: "settings"; text: string };

export function slashHits(draft: string): Promise<SlashHit[]> {
  return invoke<SlashHit[]>("slash_hits", { draft });
}

/** `conversationId` is the chat the line was typed in. */
export function runSlash(
  line: string,
  conversationId?: string,
): Promise<SlashEffect> {
  return invoke<SlashEffect>("run_slash", { line, conversationId });
}

export function addMemory(
  text: string,
  global: boolean,
): Promise<{ status: string; notes: MemoryNotes }> {
  return invoke("add_memory", { text, global });
}

export function searchMemory(query: string): Promise<MemoryHit[]> {
  return invoke<MemoryHit[]>("search_memory", { query });
}

export function editMemoryRow(id: string, text: string): Promise<MemoryNotes> {
  return invoke<MemoryNotes>("edit_memory_row", { id, text });
}

export function forgetMemoryRow(id: string): Promise<MemoryNotes> {
  return invoke<MemoryNotes>("forget_memory_row", { id });
}

export function approveMemoryRow(id: string): Promise<MemoryNotes> {
  return invoke<MemoryNotes>("approve_memory_row", { id });
}

export function rejectMemoryRow(id: string): Promise<MemoryNotes> {
  return invoke<MemoryNotes>("reject_memory_row", { id });
}

export function setMemoryWrites(on: boolean): Promise<MemoryNotes> {
  return invoke<MemoryNotes>("set_memory_writes", { on });
}

/** Keep a learned line but stop using it, or use it again. */
export function pauseMemoryRow(
  id: string,
  paused: boolean,
): Promise<MemoryNotes> {
  return invoke<MemoryNotes>("pause_memory_row", { id, paused });
}

/** The groups a line can be added to: the key sent, and the name shown. */
export const PERSONAL_ADD_GROUPS = [
  ["about", "About you"],
  ["likes", "What you like"],
  ["routines", "Your routines"],
  ["needs", "What you need"],
  ["thinking", "How you like things done"],
] as const;

export type PersonalGroupKey = (typeof PERSONAL_ADD_GROUPS)[number][0];

/** Add a line about you in your own words. */
export function addPersonalFact(
  text: string,
  group: PersonalGroupKey,
): Promise<MemoryNotes> {
  return invoke<MemoryNotes>("add_personal_fact", { text, group });
}

/** Saves one readable copy in Downloads; returns where. Desktop only. */
export function exportPersonal(): Promise<string> {
  return invoke<string>("export_personal");
}

export interface PersonalWiped {
  removed: number;
  /** Private lines that were locked and stay until opened. */
  locked: number;
  notes: MemoryNotes;
}

/** Deletes every learned line for good. Desktop only. */
export function wipePersonal(): Promise<PersonalWiped> {
  return invoke<PersonalWiped>("wipe_personal");
}

/** SOUL / USER / MEMORY tabs in the config memory folder. */
export interface MemoryFiles {
  soul: string;
  user: string;
  memory: string;
  soulPath: string;
  userPath: string;
  memoryPath: string;
  soulPrev: boolean;
  userPrev: boolean;
  memoryPrev: boolean;
  /** Old chat lines still in MEMORY.md, offered once for removal. */
  rawLines: string[];
  rawTidyDone: boolean;
}

export type MemoryTab = "SOUL.md" | "USER.md" | "MEMORY.md";

export function getMemoryFiles(): Promise<MemoryFiles> {
  return invoke<MemoryFiles>("get_memory_files");
}

export function saveMemoryFile(
  name: MemoryTab,
  text: string,
  base: string,
): Promise<{ files: MemoryFiles; redacted: boolean }> {
  return invoke("save_memory_file", { name, text, base });
}

export function restoreMemoryFile(name: MemoryTab): Promise<MemoryFiles> {
  return invoke<MemoryFiles>("restore_memory_file", { name });
}

export interface ReflectResult {
  status: string;
  diff: string;
  files: MemoryFiles;
}

/** Reflect over recent chat; returns what was written. */
export function reflectMemory(conversationId?: string): Promise<ReflectResult> {
  return invoke<ReflectResult>("reflect_memory", { conversationId });
}

export function tidyMemoryLedger(): Promise<MemoryFiles> {
  return invoke<MemoryFiles>("tidy_memory_ledger");
}

export function applyMemoryRawTidy(): Promise<MemoryFiles> {
  return invoke<MemoryFiles>("apply_memory_raw_tidy");
}

export function dismissMemoryRawTidy(): Promise<MemoryFiles> {
  return invoke<MemoryFiles>("dismiss_memory_raw_tidy");
}

export interface DaysChoice {
  days: number;
  label: string;
}

export interface MemoryRetention {
  days: number;
  label: string;
  choices: DaysChoice[];
}

export function getMemoryRetention(): Promise<MemoryRetention> {
  return invoke<MemoryRetention>("get_memory_retention");
}

export function setMemoryRetention(days: number): Promise<MemoryRetention> {
  return invoke<MemoryRetention>("set_memory_retention", { days });
}

export interface ChatArchive {
  days: number;
  label: string;
  choices: DaysChoice[];
}

export function getChatArchive(): Promise<ChatArchive> {
  return invoke<ChatArchive>("get_chat_archive");
}

export function setChatArchive(days: number): Promise<ChatArchive> {
  return invoke<ChatArchive>("set_chat_archive", { days });
}

/** One MCP server from `mcp.json`, as the Connectors page shows it. */
export interface ConnectorRow {
  name: string;
  /** "connected", "error", "stopped", "not checked" or "disabled". */
  status: string;
  tools: number;
  error: string;
  /** The command or URL the entry runs. */
  detail: string;
  /** "Signed in to … as …", empty when not signed in. */
  account: string;
  needsSignIn: boolean;
  canDisconnect: boolean;
  /** False once its switch turned it off. */
  enabled: boolean;
}

export interface ConnectorsChanged {
  status: string;
  rows: ConnectorRow[];
}

export function listConnectors(): Promise<ConnectorRow[]> {
  return invoke<ConnectorRow[]>("list_connectors");
}

/** Starts each enabled server and pings it; can take a few seconds. */
export function checkConnectors(): Promise<ConnectorRow[]> {
  return invoke<ConnectorRow[]>("check_connectors");
}

export function restartConnector(name: string): Promise<ConnectorsChanged> {
  return invoke<ConnectorsChanged>("restart_connector", { name });
}

export function signOutConnector(name: string): Promise<ConnectorsChanged> {
  return invoke<ConnectorsChanged>("sign_out_connector", { name });
}

/** Only after the person confirmed it on the page. */
export function disconnectConnector(name: string): Promise<ConnectorsChanged> {
  return invoke<ConnectorsChanged>("disconnect_connector", { name });
}

/** One connector in the Add sheet's catalog. */
export interface CatalogRow {
  id: string;
  name: string;
  publisher: string;
  category: string;
  description: string;
  auth: "none" | "oauth" | "key" | "google" | "own";
  keyHint: string;
  target: string;
  installed: boolean;
}

export interface CatalogView {
  rows: CatalogRow[];
  /** What Refresh did, when it ran. */
  note: string | null;
}

/** A key waiting on its approval card. */
export interface KeyCard {
  id: string;
  name: string;
  action: string;
}

/** `refresh` reads the official registry; only from the Refresh click. */
export function connectorCatalog(refresh = false): Promise<CatalogView> {
  return invoke<CatalogView>("connector_catalog", { refresh });
}

export function addCatalogConnector(id: string): Promise<ConnectorsChanged> {
  return invoke<ConnectorsChanged>("add_catalog_connector", { id });
}

/** A pasted `mcpServers` config; names already set up are kept. */
export function addConnectorConfig(text: string): Promise<ConnectorsChanged> {
  return invoke<ConnectorsChanged>("add_connector_config", { text });
}

export function setConnectorEnabled(
  name: string,
  on: boolean,
): Promise<ConnectorsChanged> {
  return invoke<ConnectorsChanged>("set_connector_enabled", { name, on });
}

/** Opens the server's sign-in page in the browser and waits for it. */
export function signInConnector(name: string): Promise<ConnectorsChanged> {
  return invoke<ConnectorsChanged>("sign_in_connector", { name });
}

/** Posts the approval card; nothing is saved until `answerConnectorKey`. */
export function saveConnectorKey(name: string, key: string): Promise<KeyCard> {
  return invoke<KeyCard>("save_connector_key", { name, key });
}

export function answerConnectorKey(
  id: string,
  approve: boolean,
): Promise<ConnectorsChanged> {
  return invoke<ConnectorsChanged>("answer_connector_key", { id, approve });
}

/** One Feed post: something that happened or was found. */
export interface PulsePost {
  id: string;
  source: string;
  title: string;
  summary: string;
  /** Why a fetched post is here, in words ("You play Helldivers 2"). */
  because: string;
  /** A written post's topic emoji. Empty on older posts. */
  icon: string;
  /** A written post's "why you're seeing this" line. */
  why: string;
  /** Cached gallery image names; read each with `pulseImage`. */
  images: string[];
  /** Unix ms, for the menu's full date. */
  createdAt: number;
  /** "2h ago". */
  ago: string;
  /** The first source link, empty when there is none. */
  link: string;
  loved: boolean;
  /** What the card's numbers mean, when it has a read of them. */
  verdict: PulseVerdict | null;
  /** The full report and the facts behind it (markdown). Empty when the
   * card has nothing past its summary. */
  more: string;
  /** A digest's updates, each with the card it opens. */
  items: PulseItem[];
  /** The chips under the card: its fixes, Discuss, Ignore for now, Remind me later. */
  chips: PulseChip[];
  /** Offer Snooze in the row (time-bound card, or unsure). */
  snooze: boolean;
  /** Offer Delete in the row (stale, repeat or finished card, or unsure). */
  delete: boolean;
  /** Offer Save in the row: a source post with a link. */
  save: boolean;
  /** Already on the Saved page. */
  saved: boolean;
}

/** A card kept with Save, on the Saved page. Stored apart from the feed
 * (saved-finds.json), so feed cleanup never touches it. */
export interface SavedFind {
  title: string;
  url: string;
  sourceName: string;
  summary: string;
  because: string | null;
  savedMs: number;
}

/** The Saved page: every saved card, newest first. */
export function pulseSaved(): Promise<SavedFind[]> {
  return invoke<SavedFind[]>("pulse_saved");
}

/** Remove from saved, the one action a saved card has. Returns what is left. */
export function pulseUnsave(url: string): Promise<SavedFind[]> {
  return invoke<SavedFind[]>("pulse_unsave", { url });
}

/** One update inside a digest card. */
export interface PulseItem {
  cardId: string;
  found: string;
  summary: string;
}

/** One chip under a card. `index` is the fix's place on the card. */
export interface PulseChip {
  label: string;
  act: "offer" | "discuss" | "ignore" | "remind";
  index: number;
}

/** What a card puts in the chat: its own conversation and the words for the
 * box. The card's report rides along with the first message, unseen. */
export interface PulseSeed {
  conversationId: string;
  text: string;
}

/** Remind me later: the page after, and when the card is back. */
export interface PulseReminded {
  page: PulsePage;
  note: string;
}

/** A card's one-line read of its own numbers, and how worried to be. */
export interface PulseVerdict {
  tone: "fine" | "watch" | "act";
  line: string;
}

/** One Ideas row: something GrokHub could do next. */
export interface PulseIdea {
  id: string;
  /** The bold line: a written idea's own title, else "I can …". */
  title: string;
  /** A written idea's topic emoji; empty on older ideas. */
  icon: string;
  summary: string;
  /** Do, Automate, Learn or Watch. */
  kind: string;
  /** What Let's do it puts in the chat box. */
  action: string;
  verdict: PulseVerdict | null;
  /** "Fix it" or "Options" on a PC health card: its main button opens the
   * options window. Empty on other ideas. */
  fix: string;
  /** A card that found something: its full report, opened under the summary. */
  more: string;
  /** A card that found something: its chips in place of the two buttons.
   * Empty on other ideas. */
  chips: PulseChip[];
  /** Like was tapped and is still on: the row shows a filled heart. */
  loved: boolean;
}

export type FixKind =
  | "compare"
  | "fpsCap"
  | "upscale"
  | "airflow"
  | "fanCurve"
  | "repaste"
  | "quietWhenNormal"
  | "warnLines"
  | "pause"
  | "ask";

export interface FixOption {
  kind: FixKind;
  title: string;
  why: string;
  /** Steps the person follows; empty for rows that act in the app. */
  steps: string[];
}

export interface CompareRow {
  session: string;
  time: string;
  gpuPeak: string;
  cpuPeak: string;
  gpuBusy: string;
}

/** A PC health card's options window. */
export interface FixView {
  title: string;
  verdict: PulseVerdict | null;
  options: FixOption[];
  compare: CompareRow[];
}

export interface PulsePage {
  feed: PulsePost[];
  /** How many of `feed` are today's set; "That's all for today" follows them. */
  today: number;
  /** What the feed writer's last run did, shown on an empty feed. */
  note: string;
  /** An empty label is the unlabeled top group. */
  ideas: { label: string; ideas: PulseIdea[] }[];
}

export type PulseAct =
  | "dismiss"
  | "notThis"
  | "dislike"
  | "love"
  | "unlove"
  | "delete"
  | "snooze"
  | "opened"
  | "accept"
  | "moreLike"
  | "notInterested"
  | "ignore"
  | "save"
  | "dwell";

/** Today's local date for the ledger line, and the time Snooze counts from. */
export function localDay(now: Date = new Date()) {
  const pad = (n: number) => String(n).padStart(2, "0");
  return {
    date: `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`,
    hour: now.getHours(),
    minute: now.getMinutes(),
  };
}

/** Opening Pulse also tells the Feed writer the local clock (quiet hours). */
export function getPulse(): Promise<PulsePage> {
  return invoke<PulsePage>("get_pulse", { day: localDay() });
}

export function pulseAct(id: string, act: PulseAct): Promise<PulsePage> {
  return invoke<PulsePage>("pulse_act", { id, act, day: localDay() });
}

/** Discuss: the card's own chat and the words for its box. */
export function pulseDiscuss(id: string): Promise<PulseSeed> {
  return invoke<PulseSeed>("pulse_discuss", { id });
}

/** A card's fix chip: its words in the card's own chat. Nothing runs until sent. */
export function pulseOffer(id: string, index: number): Promise<PulseSeed> {
  return invoke<PulseSeed>("pulse_offer", { id, index });
}

/** Remind me later: hidden until a time that fits the day. */
export function pulseRemind(id: string): Promise<PulseReminded> {
  return invoke<PulseReminded>("pulse_remind", { id, day: localDay() });
}

export function pulseWhy(id: string): Promise<string> {
  return invoke<string>("pulse_why", { id });
}

/** A PC health card's options window. Read only. */
export function pulseFix(id: string): Promise<FixView> {
  return invoke<FixView>("pulse_fix", { id });
}

/** The Feed prompt the writer follows, and what its last run did. */
export interface FeedPrompt {
  text: string;
  isDefault: boolean;
  note: string;
}

export function getFeedPrompt(): Promise<FeedPrompt> {
  return invoke<FeedPrompt>("get_feed_prompt");
}

/** Saving writes one new post right away. */
export function saveFeedPrompt(text: string): Promise<FeedPrompt> {
  return invoke<FeedPrompt>("save_feed_prompt", { text });
}

/** A cached gallery image as a data URL; pages never load it from its site. */
export function pulseImage(name: string): Promise<string> {
  return invoke<string>("pulse_image", { name });
}

/** The Imagine page (src-tauri/src/imagine.rs). Its gallery is the cabin's wall. */
export type ImagineKind = "image" | "video";

export interface ImagineTile {
  id: string;
  /** Read it with `imagineImage`. */
  name: string;
  title: string;
  prompt: string;
  createdMs: number;
  video: boolean;
  tall: boolean;
}

export interface ImaginePage {
  /** Newest first. */
  tiles: ImagineTile[];
  /** A Grok sign-in or a key is there, so Make can run. */
  ready: boolean;
}

export interface ImagineMade {
  page: ImaginePage;
  made: number;
  /** Empty, or which results could not be saved. */
  note: string;
}

export function getImagine(): Promise<ImaginePage> {
  return invoke<ImaginePage>("get_imagine");
}

/** Resolves once the results are saved; `imagineCancel` ends it early. */
export function imagineGenerate(
  prompt: string,
  aspect: string,
  count: number,
  kind: ImagineKind,
): Promise<ImagineMade> {
  return invoke<ImagineMade>("imagine_generate", {
    prompt,
    aspect,
    count,
    kind,
  });
}

export function imagineCancel(): Promise<boolean> {
  return invoke<boolean>("imagine_cancel");
}

/** A gallery file as a data URL, by its bare name. */
export function imagineImage(name: string): Promise<string> {
  return invoke<string>("imagine_image", { name });
}

/** One decision journal question (src-tauri/src/journal.rs). */
export interface JournalQuestion {
  id: string;
  question: string;
  /** "Open", "Investigating", "Ready to decide", "Deferred", … */
  status: string;
  active: boolean;
  options: { id: string; label: string }[];
  /** The id of the option it leans to. */
  leaning: string | null;
  /** The option label picked when decided. */
  chosen: string | null;
  /** The chat its mission runs in. */
  mission: string | null;
}

export interface JournalPage {
  active: JournalQuestion[];
  history: JournalQuestion[];
}

export type JournalAct =
  | { kind: "addOption"; label: string }
  | { kind: "lean"; optionId: string }
  | { kind: "defer" }
  | { kind: "abandon" }
  | { kind: "reopen" }
  | { kind: "decide"; optionId: string | null };

/** What To mission puts in the chat box, and the chat it goes to. */
export interface JournalMission {
  conversationId: string;
  title: string;
  prompt: string;
}

export function getJournal(): Promise<JournalPage> {
  return invoke<JournalPage>("get_journal");
}

export function journalAdd(question: string): Promise<JournalPage> {
  return invoke<JournalPage>("journal_add", { question });
}

export function journalAct(id: string, act: JournalAct): Promise<JournalPage> {
  return invoke<JournalPage>("journal_act", { id, act });
}

export function journalToMission(
  id: string,
  conversationId: string,
): Promise<JournalMission> {
  return invoke<JournalMission>("journal_to_mission", { id, conversationId });
}

/** The workboard (src-tauri/src/workboard.rs), from the cabin's workboard.json. */
export type BoardLane = "followUp" | "todo" | "doing" | "blocked" | "done";

export interface WorkStepView {
  text: string;
  status: "planned" | "doing" | "done" | "skipped" | "failed";
}

export interface BoardCard {
  id: string;
  title: string;
  detail: string;
  /** Your notes for the agent; Work hands them over. */
  notes: string;
  priority: string;
  /** A new report you have not opened yet. */
  fresh: boolean;
  /** A Follow up card's latest report. */
  report: string;
  /** What the card's last background run came back with. */
  result: string;
  /** Who owns it, as the cabin's badge reads. */
  owner: "Agent" | "You";
  /** Automation, Background, Episode or Flow; a plain task has none. */
  kind: string | null;
  /** Where the work left off. */
  leftOff: string | null;
  steps: WorkStepView[];
  checkpoint: {
    goal: string;
    next: string;
    blocker: string;
    refs: string[];
  } | null;
  /** Steps done and in all. */
  progress: [number, number];
  /** A paused or interrupted run: Resume and Hand back apply. */
  canResume: boolean;
  /** The cabin chat that works it. */
  chat: string | null;
  updatedMs: number;
  /** When it was archived; 0 for a card on the board. */
  archivedMs: number;
  /** "Queued", "Queued · 2nd": waiting for the agent; null when not queued. */
  queued: string | null;
  /** The flow started on it: "Playbook · Linux repair". */
  flow: string | null;
  /** You edited the steps, so the agent works from your version. */
  userSteps: boolean;
  /** The steps as the editor shows them, one a line with its mark. */
  stepsText: string;
}

export interface BoardColumn {
  lane: BoardLane;
  label: string;
  cards: BoardCard[];
}

export interface BoardPage {
  /** Follow up, then Todo, Doing, Blocked, Done. */
  columns: BoardColumn[];
  archived: BoardCard[];
  /** The file's mtime, for the change check. */
  stamp: number;
  /** The last sweep's notice, until Undo or Dismiss: "Archived 2 finished cards". */
  swept: BoardSwept | null;
  /** Cards wait in the queue and the cabin, which starts them, is closed. */
  queueNote: string | null;
}

export interface BoardSwept {
  text: string;
  ids: string[];
  at: number;
}

/** Settings → Workboard tidy. */
export interface BoardTidy {
  /** Days with no activity before a card is archived. */
  autoArchiveDays: number;
  /** 0 is off (the default); else 30 or 90. */
  autoDeleteDays: number;
}

export type BoardAct =
  | { kind: "edit"; title: string; detail: string }
  | { kind: "notes"; notes: string }
  | { kind: "move"; to: BoardLane }
  | { kind: "rank"; up: boolean }
  | { kind: "handBack" }
  | { kind: "archive" }
  | { kind: "restore" }
  | { kind: "delete" }
  | { kind: "queue" }
  | { kind: "unqueue" }
  | { kind: "priority"; priority: "high" | "normal" | "low" }
  | { kind: "block"; reason: string }
  | { kind: "unblock" }
  | { kind: "steps"; text: string }
  | { kind: "place"; to: BoardLane; before: string | null };

/** Make it recurring: the schedules offered, in the order the shell's
 * `work_triage::RECUR_CHOICES` keeps them. */
export const RECUR_CHOICES = [
  "Every day at 9 AM",
  "Every weekday at 9 AM",
  "Every day at 6 PM",
] as const;

export interface WorkLogLine {
  at: number;
  kind: string;
  text: string;
}

/** What Work puts in the chat box, and the board after the card moved. */
export interface BoardWork {
  prompt: string;
  page: BoardPage;
}

/** What Resume puts in the chat box, and the chat it goes to. */
export interface BoardResume {
  conversationId: string;
  prompt: string;
  page: BoardPage;
}

export function getWorkboard(): Promise<BoardPage> {
  return invoke<BoardPage>("get_workboard");
}

/** The board file's mtime; cheap enough to poll. */
export function workboardStamp(): Promise<number> {
  return invoke<number>("workboard_stamp");
}

/** A card's progress log tail. Read only when you open its log. */
export function workboardLog(id: string): Promise<WorkLogLine[]> {
  return invoke<WorkLogLine[]>("workboard_log", { id });
}

export function workboardAdd(
  title: string,
  detail: string,
): Promise<BoardPage> {
  return invoke<BoardPage>("workboard_add", { title, detail });
}

export function workboardAct(id: string, act: BoardAct): Promise<BoardPage> {
  return invoke<BoardPage>("workboard_act", { id, act });
}

export function workboardWork(id: string): Promise<BoardWork> {
  return invoke<BoardWork>("workboard_work", { id });
}

/** Start a playbook on a card: it links the playbook, then starts as Work. */
export function workboardPlaybook(
  id: string,
  playbook: string,
  name: string,
): Promise<BoardWork> {
  return invoke<BoardWork>("workboard_playbook", { id, playbook, name });
}

/** Make it recurring: an automation from the card, `choice` in RECUR_CHOICES. */
export function workboardRecur(id: string, choice: number): Promise<BoardPage> {
  return invoke<BoardPage>("workboard_recur", {
    id,
    choice,
    clock: wallClock(),
  });
}

/** Delete archived cards archived `days` ago or more (0: all of them), with
 * their logs. Cards on the board are never touched. */
export function workboardDeleteArchived(days: number): Promise<BoardPage> {
  return invoke<BoardPage>("workboard_delete_archived", { days });
}

/** Undo on the sweep notice: its cards go back to Todo. */
export function workboardSweepUndo(): Promise<BoardPage> {
  return invoke<BoardPage>("workboard_sweep_undo");
}

export function workboardSweepDismiss(): Promise<BoardPage> {
  return invoke<BoardPage>("workboard_sweep_dismiss");
}

export function getWorkboardTidy(): Promise<BoardTidy> {
  return invoke<BoardTidy>("get_workboard_tidy");
}

export function setWorkboardTidy(autoDeleteDays: number): Promise<BoardTidy> {
  return invoke<BoardTidy>("set_workboard_tidy", { autoDeleteDays });
}

/** Resume a paused run. A stub until KB-03 fills in the real resume. */
export function workboardResume(
  id: string,
  conversationId: string,
): Promise<BoardResume> {
  return invoke<BoardResume>("workboard_resume", { id, conversationId });
}

/** Home and Objectives (src-tauri/src/home.rs): the cabin's tiles from
 * home_layout.json, and the open chat's checklist with level and XP. */
export type StepStatus =
  "planned" | "attempted" | "verified" | "unverified" | "cancelled";

export interface MissionStep {
  id: string;
  text: string;
  status: StepStatus;
}

export interface Mission {
  conversationId: string;
  title: string;
  /** active, needs_approval, paused, blocked or done. */
  status: string;
  steps: MissionStep[];
  done: number;
  total: number;
  /** Every step is closed and the result is not accepted yet. */
  canAccept: boolean;
  /** The newest outcome is a success, so Undo has something to undo. */
  accepted: boolean;
}

/** This week on the local clock, Monday first. */
export interface WeekView {
  /** Monday to Sunday: true for a day with any XP. */
  days: boolean[];
  /** Today's place in `days`. */
  today: number;
  /** Days with XP so far this week. */
  active: number;
  /** Active days the week asks for, tuned from the weeks before. */
  target: number;
  inARow: number;
  /** Weeks with any XP, ever. */
  played: number;
  freezes: number;
}

export interface Xp {
  level: number;
  /** "Rookie", "Grinder" and so on, a new one every five levels. */
  title: string;
  xp: number;
  /** Total XP at which the next level starts. */
  nextLevelXp: number;
  /** Progress into the level, 0 to 1. */
  frac: number;
  today: number;
  week: WeekView;
}

export interface MissionPage {
  mission: Mission | null;
  xp: Xp;
}

export type HomeTile =
  | { kind: "objectives"; mission: Mission | null; xp: Xp; followUp?: FollowUp }
  | { kind: "streak"; xp: Xp }
  | {
      kind: "recentChats";
      chats: { id: string; title: string; updated: number }[];
    }
  | {
      kind: "automations";
      active: number;
      next: { id: string; name: string; summary: string }[];
    }
  | {
      kind: "pulse";
      post: { id: string; title: string; summary: string; ago: string } | null;
      idea: { id: string; title: string; action: string } | null;
    }
  | { kind: "personality"; name: string; picture: string | null }
  | { kind: "upNext"; games: { appId: number; name: string }[] }
  | { kind: "error"; message: string }
  /** Any other module, in the shared tile model. */
  | { kind: "view"; view: TileView };

export type TileTone = "normal" | "warn" | "alert" | "quiet";
export type TileState =
  "ready" | "empty" | "off" | "error" | "offline" | "unsupported";

/** What a tile, row or button does; Home maps it onto its routes. */
export type TileAction =
  | { kind: "ask"; ask: string }
  | { kind: "openChat"; id: string }
  | { kind: "nav"; page: string }
  | { kind: "settings"; section: string }
  | { kind: "runPlaybook"; id: string }
  | { kind: "review"; id: string }
  | { kind: "toggle"; feature: string; on: boolean }
  | { kind: "turnOn"; feature: string }
  | { kind: "openUrl"; url: string }
  | { kind: "play"; appId: number }
  | { kind: "openClip"; index: number }
  | { kind: "steamFriends" }
  | { kind: "steamSettings" }
  | { kind: "setup"; module: string; undo: boolean }
  | { kind: "copySetup"; module: string }
  | { kind: "reroll" }
  | { kind: "retry" };

export interface TileView {
  state: TileState;
  tag: string | null;
  text: { big: string; caption: string; detail: string | null; tone: TileTone };
  rows: {
    label: string;
    value: string;
    tone: TileTone;
    tap: TileAction | null;
  }[];
  progress: number | null;
  buttons: { label: string; action: TileAction; primary: boolean }[];
  tap: TileAction | null;
  updatedMs: number | null;
}

export type SlotSize = "large" | "medium" | "small";

export interface HomeSlot {
  /** "large", "medium-1" … "small-4". */
  slot: string;
  size: SlotSize;
  module: string;
  name: string;
  purpose: string;
  tile: HomeTile;
  /** How often this tile's data changes, in seconds; `null` if it doesn't on its own. */
  refreshSecs: number | null;
}

export interface HomePage {
  preset: string;
  xpBar: boolean;
  slots: HomeSlot[];
  xp: Xp;
  glance: {
    memories: number | null;
    proposed: number;
    connectors: number;
    connected: number;
    failing: number;
  };
}

export function getHome(conversationId?: string): Promise<HomePage> {
  return invoke<HomePage>("get_home", { clock: wallClock(), conversationId });
}

/** Which module sits in each slot. "" or null hides a Medium or Small slot. */
export interface HomePick {
  preset: string;
  large: string;
  medium: string[];
  small: (string | null)[];
}

export interface HomeCatalog {
  current: HomePick;
  modules: {
    id: string;
    name: string;
    category: string;
    sizes: SlotSize[];
    purpose: string;
  }[];
  presets: { id: string; label: string; pick: HomePick }[];
}

export function homeCatalog(): Promise<HomeCatalog> {
  return invoke<HomeCatalog>("home_catalog");
}

export function setHomeLayout(pick: HomePick): Promise<void> {
  return invoke<void>("set_home_layout", { pick });
}

export function getMission(conversationId?: string): Promise<MissionPage> {
  return invoke<MissionPage>("get_mission", {
    clock: wallClock(),
    conversationId,
  });
}

/** Accept as done (`done`) or Undo. Desktop only. */
export function setMissionDone(
  done: boolean,
  conversationId?: string,
): Promise<MissionPage> {
  return invoke<MissionPage>("set_mission_done", {
    clock: wallClock(),
    conversationId,
    done,
  });
}

/** One goal row on Objectives (grokhub-core objectives::GoalCard). */
export interface GoalCard {
  id: string;
  /** "Old School RuneScape · 99 Fishing". */
  title: string;
  /** "93 / 99 · 6 levels to 99". */
  detail: string;
  frac: number;
  /** True when the goal moves one step at a time and gets a +1. */
  plusOne: boolean;
  /** "Pick up Elden Ring tonight?" on a Today row. */
  prompt?: string;
  /** "Nov 2" on a finished goal. */
  doneOn?: string;
}

/** A gentle follow-up on a goal left a few days (grokhub-core objectives::FollowUp). */
export interface FollowUp {
  id: string;
  /** "Elden Ring · Finish". */
  title: string;
  /** "Want to pick a night for your next Elden Ring session?", or the last line. */
  line: string;
  /** What "Plan it" puts in the chat box. Empty on the last line. */
  prompt: string;
  /** "I'll stop asking about …": a note with no button. */
  stop: boolean;
}

export interface GoalBoard {
  today: GoalCard[];
  mine: GoalCard[];
  suggested: GoalCard[];
  season: {
    label: string;
    done: number;
    trophies: GoalCard[];
    /** Seasons whose badge was earned (three goals done), newest first. */
    badges: string[];
    /** "1 more goal for the Oct to Dec 2026 badge"; absent once earned. */
    badgeLine?: string;
    /** "First goal this season: +50 bonus XP", for a week after it landed. */
    bonus?: string;
  };
  followUp?: FollowUp;
  /** Monday's "Your week": up to three real changes and one goal to try. */
  week?: YourWeek;
}

export interface YourWeek {
  lines: string[];
  suggestion?: GoalCard;
}

export type GoalAct =
  | { act: "add"; text: string }
  | { act: "bump" | "done" | "drop" | "track" | "notNow"; id: string };

export interface GoalsSaved {
  board: GoalBoard;
  /** Why an added goal was refused. */
  note?: string;
}

export function getGoals(): Promise<GoalBoard> {
  return invoke<GoalBoard>("get_goals", { clock: wallClock() });
}

export function goalAct(acts: GoalAct[]): Promise<GoalsSaved> {
  return invoke<GoalsSaved>("goal_act", { clock: wallClock(), acts });
}

/** Settings → Local AI (src-tauri/src/local_ai.rs). */
export type LocalMode = "auto" | "manual";

export interface LocalCaps {
  rampUp: number;
  normal: number;
  throttle: number;
}

export interface LocalManual {
  /** "auto", "cuda", "rocm", "vulkan", "metal", "sycl" or "cpu". */
  backend: string;
  ngl: number | null;
  ctx: number | null;
  threads: number | null;
  /** "auto", "q8" or "q4". */
  kv: string;
  idleMinutes: number;
  caps: LocalCaps;
  neverUnload: boolean;
  allowApps: string[];
  denyApps: string[];
}

export interface LocalAiConfig {
  mode: LocalMode;
  pauseForGames: boolean;
  useIdleTime: boolean;
  throttleOnBattery: boolean;
  manual: LocalManual;
  /** A catalog id or a model file's path; empty for the tier default. */
  localModel: { selected: string };
  /** Model files whose notice was read. */
  noticeSeen: string[];
}

export interface LocalModelRow {
  id: string;
  name: string;
  size: string;
  quant: string;
  ctx: number;
  tokS: string;
  license: string;
  badges: string[];
  bucket: "recommended" | "maySlow" | "hidden";
  reason: string;
  preselected: boolean;
  optional: boolean;
}

export interface LocalFileRow {
  path: string;
  name: string;
  size: string;
  source: string;
  thirdParty: boolean;
  abliterated: boolean;
  needsNotice: boolean;
}

/** `state_word` and the not-set-up line in `local_ai.rs`. */
export type LocalStatusState =
  "Normal" | "Throttled" | "Ramping up" | "Paused" | "Not set up";

export interface LocalStatus {
  tier: string;
  model: string;
  state: LocalStatusState;
  reason: string;
  tokS: string;
  memory: string;
}

export interface LocalEngineProgress {
  done: number;
  total: number | null;
  verifying: boolean;
  line: string;
}

/** `EngineInfo.state` in `local_engine.rs`. */
export type LocalEngineState = "ready" | "missing" | "downloading" | "noBuild";
/** What the engine download emits: it never reports `noBuild`. */
export type LocalEngineEventState = "downloading" | "ready" | "missing";
/** What the draft model download emits. */
export type LocalDrafterState =
  "downloading" | "done" | "error" | "paused" | "cancelled";

export interface LocalEngineInfo {
  state: LocalEngineState;
  version: string;
  sizeLabel: string;
  hint: string;
  button: string;
  progress: LocalEngineProgress | null;
  error: string | null;
}

export interface LocalDraftOffer {
  id: string;
  name: string;
  sizeLabel: string;
  line: string;
  button: string;
  targetId: string;
  targetName: string;
}

export interface LocalDraftProgress {
  id: string;
  done: number;
  total: number | null;
  verifying: boolean;
  line: string;
}

export interface LocalAiView {
  config: LocalAiConfig;
  status: LocalStatus;
  picker: {
    models: LocalModelRow[];
    onThisPc: LocalFileRow[];
    selected: string;
  };
  /** What hardware detection couldn't see. */
  limits: string[];
  maxThreads: number;
  engine: LocalEngineInfo;
  draftOffer: LocalDraftOffer | null;
  /** Offers for every catalog model still missing its draft pair. */
  draftOffers: LocalDraftOffer[];
  draftProgress: LocalDraftProgress | null;
  running: LocalRunning;
}

/** What runs now, with its two controls: keep loaded and unload now. */
export interface LocalRunning {
  lines: string[];
  loaded: boolean;
  keptLoaded: boolean;
  /** Dynamic: the best model on disk per task. */
  dynamic: boolean;
  dynamicHint: string;
}

export type LocalEngineEvent = {
  state: LocalEngineEventState;
  progress: LocalEngineProgress | null;
  error: string | null;
  message: string | null;
};

export type LocalDrafterEvent = {
  state: LocalDrafterState;
  progress: LocalDraftProgress | null;
  error: string | null;
  message: string | null;
};

/** Reads the hardware, so it can take a moment. */
export function getLocalAi(): Promise<LocalAiView> {
  return invoke<LocalAiView>("get_local_ai");
}

export function saveLocalAi(config: LocalAiConfig): Promise<LocalAiView> {
  return invoke<LocalAiView>("save_local_ai", { config });
}

export function resetLocalAi(): Promise<LocalAiView> {
  return invoke<LocalAiView>("reset_local_ai");
}

export function checkModelFile(path: string): Promise<LocalFileRow> {
  return invoke<LocalFileRow>("check_model_file", { path });
}

export function setLocalKeepLoaded(on: boolean): Promise<LocalAiView> {
  return invoke<LocalAiView>("set_local_keep_loaded", { on });
}

export function setLocalDynamic(on: boolean): Promise<LocalAiView> {
  return invoke<LocalAiView>("set_local_dynamic", { on });
}

export function unloadLocalModel(): Promise<LocalAiView> {
  return invoke<LocalAiView>("unload_local_model");
}

/** Settings → Local AI → Download models (src-tauri/src/model_library.rs). */
export type LibraryRowState =
  | "download"
  | "resume"
  | "queued"
  | "downloading"
  | "downloaded"
  | "inUse"
  | "wontRun";

export interface LibraryRow {
  id: string;
  name: string;
  line: string;
  about: string;
  state: LibraryRowState;
}

export interface LibraryQueueOffer {
  ids: string[];
  line: string;
}

export interface LibraryTab {
  label: string;
  rows: LibraryRow[];
  empty: string | null;
  all: LibraryQueueOffer | null;
}

/** Why a model's last download stopped; it shows under that model's row. */
export interface LibraryRowError {
  /** free space, checksum read, download, verify or engine. */
  step: string;
  message: string;
  /** The step, plus free and needed space when space is why. */
  detail: string;
}

export interface LibraryStatus {
  job: { id: string; line: string; pct: number } | null;
  queued: number;
  message: string | null;
  /** By model id, until that model is downloaded again or finishes. */
  errors: Record<string, LibraryRowError>;
  running: boolean;
}

export interface LibraryView {
  tabs: LibraryTab[];
  /** The best lineup for this PC, one model per goal for hot-swap; `ids` may be empty when it is all downloaded. */
  lineup: LibraryQueueOffer | null;
  everything: LibraryQueueOffer | null;
  status: LibraryStatus;
}

/** Reads the hardware, so it can take a moment. */
export function getModelLibrary(): Promise<LibraryView> {
  return invoke<LibraryView>("get_model_library");
}

export function downloadModels(ids: string[]): Promise<LibraryView> {
  return invoke<LibraryView>("download_models", { ids });
}

export function pauseModelDownloads(): Promise<void> {
  return invoke<void>("pause_model_downloads");
}

export function removeLibraryModel(id: string): Promise<LibraryView> {
  return invoke<LibraryView>("remove_library_model", { id });
}

export function onModelLibraryEvent(
  handler: (status: LibraryStatus) => void,
): Promise<UnlistenFn> {
  return subscribe<LibraryStatus>("model-library", handler);
}

export function runLocalSpeedTest(): Promise<string> {
  return invoke<string>("run_local_speed_test");
}

export function startLocalEngineDownload(): Promise<LocalEngineInfo> {
  return invoke<LocalEngineInfo>("start_local_engine_download");
}

export function cancelLocalEngineDownload(): Promise<void> {
  return invoke("cancel_local_engine_download");
}

export function startLocalDrafterDownload(
  targetId: string,
): Promise<LocalDraftOffer | null> {
  return invoke<LocalDraftOffer | null>("start_local_drafter_download", {
    targetId,
  });
}

export function cancelLocalDrafterDownload(): Promise<void> {
  return invoke("cancel_local_drafter_download");
}

export function onLocalEngineEvent(
  handler: (event: LocalEngineEvent) => void,
): Promise<UnlistenFn> {
  return subscribe<LocalEngineEvent>("local-ai-engine", handler);
}

export function onLocalDrafterEvent(
  handler: (event: LocalDrafterEvent) => void,
): Promise<UnlistenFn> {
  return subscribe<LocalDrafterEvent>("local-ai-drafter", handler);
}

/** Settings → Web access (src-tauri/src/remote.rs). */
export interface WebAccess {
  on: boolean;
  /** The pairing code; empty while off. */
  code: string;
  /** Pairing links, the LAN one (HTTPS) first, then this computer's own (HTTP). */
  links: string[];
  /** SHA-256 of the HTTPS certificate, `AB:CD:…`; empty while off. */
  fingerprint: string;
}

export function getWebAccess(): Promise<WebAccess> {
  return invoke<WebAccess>("get_web_access");
}

export function setWebAccess(on: boolean): Promise<WebAccess> {
  return invoke<WebAccess>("set_web_access", { on });
}

/** A browser paired with Web access right now (src-tauri/src/remote_devices.rs). */
export interface PairedBrowser {
  /** The browser and its system, like "Firefox on Android". */
  name: string;
  /** Unix ms. */
  firstSeen: number;
  /** Unix ms. */
  lastCall: number;
  /** Chats it sent a message in. */
  chats: number;
}

/** The Devices page; desktop only, kept in memory only. */
export interface Devices {
  /** This computer's name. */
  computer: string;
  /** Whether web access is on. */
  on: boolean;
  /** The web app's LAN address without the code; empty while off. */
  lan: string;
  /** The most recently used first. */
  browsers: PairedBrowser[];
}

export function getDevices(): Promise<Devices> {
  return invoke<Devices>("get_devices");
}

/** Signs out every browser: a new pairing code ends every pairing. */
export function forgetAllDevices(): Promise<Devices> {
  return invoke<Devices>("forget_all_devices");
}

/** A computer paired with this one's LAN hub. */
export interface SharePeer {
  /** The computer's device id; two computers can share a name. */
  id: string;
  name: string;
  /** Unix ms; 0 before its first call. */
  lastSeen: number;
}

/** Devices → Start share (src-tauri/src/hub_share.rs); desktop only. */
export interface Share {
  on: boolean;
  port: number;
  /** The live pairing code while sharing; empty otherwise. Never log it. */
  code: string;
  /** What the other computer opens, without the code; empty while off. */
  address: string;
  /** With no code while on: true when a computer paired with it, false when
   * it stopped working some other way (wrong guesses burned it). */
  pairedSinceCode: boolean;
  peers: SharePeer[];
}

export function getShare(): Promise<Share> {
  return invoke<Share>("get_share");
}

export function startShare(): Promise<Share> {
  return invoke<Share>("start_share");
}

export function stopShare(): Promise<Share> {
  return invoke<Share>("stop_share");
}

export function newShareCode(): Promise<Share> {
  return invoke<Share>("new_share_code");
}

/** Settings → Labs → Plugins (src-tauri/src/plugins.rs). */
export interface PluginRow {
  name: string;
  version: string;
  /** What the bundle brings: skills, hooks, MCP servers, agents. */
  summary: string;
  trusted: boolean;
  enabled: boolean;
  /** Installed through GrokHub, so it can be removed here. */
  installed: boolean;
}

export interface PluginPage {
  rows: PluginRow[];
  marketUrl: string;
  /** What the last action did, or why it failed. */
  note: string;
}

export interface PluginEntry {
  name: string;
  version: string;
  description: string;
  source: string;
}

export interface PluginMarket {
  entries: PluginEntry[];
  page: PluginPage;
}

export interface DiagnosticsPage {
  mode: boolean;
  includeLogs: boolean;
  /** What each Save includes, the cabin's own lines. */
  preview: string[];
}

export interface DiagnosticsPreview {
  /** "Redacted: 1 key, 0 emails, …", what Copy summary copies. */
  summary: string;
  /** The redacted bundle as readable text. */
  markdown: string;
  turns: number;
}

export interface DiagnosticsSaved {
  path: string;
  status: string;
}

export function getDiagnostics(): Promise<DiagnosticsPage> {
  return invoke<DiagnosticsPage>("get_diagnostics");
}

export function setDiagnostics(mode: boolean, includeLogs: boolean): Promise<DiagnosticsPage> {
  return invoke<DiagnosticsPage>("set_diagnostics", { mode, includeLogs });
}

/** Builds the redacted bundle for Save. `chat` is the one chat whose text goes in. */
export function diagnosticsPreview(chat: string | null): Promise<DiagnosticsPreview> {
  return invoke<DiagnosticsPreview>("diagnostics_preview", { chat });
}

export function diagnosticsSave(): Promise<DiagnosticsSaved> {
  return invoke<DiagnosticsSaved>("diagnostics_save");
}

export type PluginAct = "trust" | "enable" | "disable" | "remove";

export function getPlugins(): Promise<PluginPage> {
  return invoke<PluginPage>("get_plugins");
}

export function pluginAct(name: string, act: PluginAct): Promise<PluginPage> {
  return invoke<PluginPage>("plugin_act", { name, act });
}

export function installPlugin(source: string): Promise<PluginPage> {
  return invoke<PluginPage>("install_plugin", { source });
}

export function fetchPluginMarket(url: string): Promise<PluginMarket> {
  return invoke<PluginMarket>("fetch_plugin_market", { url });
}

/** Settings → Personality (src-tauri/src/personality.rs). */
export type Emoji = "none" | "light" | "heavy";
export type Verbosity = "terse" | "normal" | "chatty";

export interface Preset {
  id: string;
  name: string;
  description: string;
  /** A reply to "my build failed" in this voice. */
  sample: string;
  toneRules: string;
  emoji: Emoji;
  verbosity: Verbosity;
  explicit: boolean;
  /** The person's own, so it can be edited and deleted. */
  custom: boolean;
}

export interface PersonalityPage {
  presets: Preset[];
  /** The default for every chat. */
  current: string;
  /** Custom files that could not be used. */
  problems: string[];
  /** Adults-only personalities: off leaves the explicit presets out. On only
   * once the 18+ check was answered yes. */
  explicit: boolean;
  /** The switch was turned on before the 18+ check: nothing changed; ask it. */
  ageCheck: boolean;
  /** How crude the jokes get while adults-only is on. */
  heat: Heat;
  /** Another voice can step in while the topic calls for it. */
  dynamicChat: boolean;
}

/** A stop on the Heat dial (src-tauri `set_quip_heat`); "clean" reads as cheeky. */
export type Heat = "clean" | "cheeky" | "spicy" | "filthy";

export interface PersonalityEdit {
  id: string;
  name: string;
  description: string;
  toneRules: string;
  emoji: Emoji;
  verbosity: Verbosity;
}

export function getPersonalities(): Promise<PersonalityPage> {
  return invoke<PersonalityPage>("get_personalities");
}

export function setPersonality(id: string): Promise<PersonalityPage> {
  return invoke<PersonalityPage>("set_personality", { id });
}

export function setExplicitPersonalities(
  on: boolean,
): Promise<PersonalityPage> {
  return invoke<PersonalityPage>("set_explicit_personalities", { on });
}

/** The one-time 18+ check's yes: remembered, and adults-only turns on. */
export function confirmAdult(): Promise<PersonalityPage> {
  return invoke<PersonalityPage>("confirm_adult");
}

export function setQuipHeat(heat: Heat): Promise<PersonalityPage> {
  return invoke<PersonalityPage>("set_quip_heat", { heat });
}

export function setDynamicChat(on: boolean): Promise<PersonalityPage> {
  return invoke<PersonalityPage>("set_dynamic_chat", { on });
}

/** The copy's id, then the page. */
export function duplicatePersonality(
  id: string,
): Promise<[string, PersonalityPage]> {
  return invoke<[string, PersonalityPage]>("duplicate_personality", { id });
}

export function savePersonality(
  edit: PersonalityEdit,
): Promise<PersonalityPage> {
  return invoke<PersonalityPage>("save_personality", { edit });
}

export function deletePersonality(id: string): Promise<PersonalityPage> {
  return invoke<PersonalityPage>("delete_personality", { id });
}

/** Settings → Playbooks (src-tauri/src/playbooks.rs). */
export interface PlaybookRow {
  id: string;
  name: string;
  summary: string;
  /** "edited": Reset puts the shipped one back. */
  source: "shipped" | "edited" | "yours";
  /** "Fits task · files", or "Fits any task". */
  fits: string;
  /** "6 steps, up to Risky, asks first". */
  steps: string;
  /** The TOML the editor shows. */
  text: string;
}

export interface PlaybookPage {
  rows: PlaybookRow[];
  /** Files of yours that could not be used. */
  problems: string[];
}

export function getPlaybooks(): Promise<PlaybookPage> {
  return invoke<PlaybookPage>("get_playbooks");
}

/** The saved playbook's name, then the page. */
export function savePlaybook(text: string): Promise<[string, PlaybookPage]> {
  return invoke<[string, PlaybookPage]>("save_playbook", { text });
}

/** The copy's id, then the page. */
export function duplicatePlaybook(id: string): Promise<[string, PlaybookPage]> {
  return invoke<[string, PlaybookPage]>("duplicate_playbook", { id });
}

export function resetPlaybook(id: string): Promise<PlaybookPage> {
  return invoke<PlaybookPage>("reset_playbook", { id });
}

/** Settings → Privacy & data → What the app learned (grokhub_core::learn::view). */
export type LearnedStage =
  "watching" | "suggesting" | "acting" | "measuring" | "wrong" | "pinned";

export interface LearnedRow {
  /** The belief's id, e.g. "card:suggestion". Forget and That's wrong take it. */
  key: string;
  title: string;
  says: string;
  /** How many times it was seen, after fading. */
  evidence: number;
  /** 0 to 100. */
  sure: number;
  stage: LearnedStage;
  lastMs: number;
}

export interface LearnedView {
  rows: LearnedRow[];
  /** Single quick chips counted but not listed (their ids are hashes). */
  singleChips: number;
  /** Learning is paused: nothing new is counted. */
  paused: boolean;
  trust: LearnedTrust;
}

export type TrustArea =
  "settings" | "skills" | "router" | "local-ai" | "proactive" | "chips";
/** How far an area may go: learn only, offer changes, or make them when sure. */
export type TrustDial = "watch" | "offer" | "act";

export interface LearnedDecision {
  atMs: number;
  /** What it was about, e.g. "Skill tidy in the morning". */
  title: string;
  /** "Did it", "Asked", "Held back" or "Stopped". */
  did: string;
  why: string;
}

export type TrustGoal = "cheapest" | "fastest" | "quiet" | "best_quality";

/** One area's dial and its scoreboard line (grokhub_core::learn::score). */
export interface TrustRow {
  area: TrustArea;
  label: string;
  dial: TrustDial;
  /** 0 to 100: the share of its offers and changes that went the user's way in the last four weeks. */
  went: number | null;
  /** The same for the four weeks before. */
  before: number | null;
  trend: "too_early" | "better" | "steady" | "worse";
  /** It got worse, so it only offers for a month. */
  resting: boolean;
}

/** The decision engine's side of the page (grokhub_core::learn::decide). */
export interface LearnedTrust {
  dials: TrustRow[];
  /** What matters most to the user, first to last. */
  goals: { goal: TrustGoal; label: string }[];
  /** What is going on right now, as the app last told the engine. */
  now: {
    quiet: boolean;
    busy: boolean;
    halted: boolean;
    gaming: boolean;
    onBattery: boolean;
  };
  /** Too much was undone lately, so everything asks first for now. */
  askingFirst: boolean;
  /** Changes the app may still make on its own today. */
  leftToday: number;
  /** Offers waiting for a yes or a no (model choice has no card of its own). */
  asks: { key: string; title: string; why: string }[];
  /** Newest first. */
  decisions: LearnedDecision[];
}

export type LearnedAct =
  | { kind: "forget"; key: string }
  | { kind: "wrong"; key: string }
  | { kind: "always"; key: string }
  | { kind: "forgetAll" }
  | { kind: "pause" }
  | { kind: "resume" }
  | { kind: "trust"; area: TrustArea; dial: TrustDial }
  | { kind: "goals"; goals: TrustGoal[] }
  | { kind: "carryOn" }
  | { kind: "answer"; key: string; yes: boolean };

export function getLearned(): Promise<LearnedView> {
  return invoke<LearnedView>("get_learned");
}

export function learnedAct(act: LearnedAct): Promise<LearnedView> {
  return invoke<LearnedView>("learned_act", { act });
}

/** The fault report, and backup and move (learn/view.rs `PAGE_LINES`). */
export type LearnedLine = "report" | "backup" | "import" | "restore";

/** Runs one of those and returns the text to show. Nothing is sent anywhere. */
export function learnedSay(line: LearnedLine): Promise<string> {
  return invoke<string>("learned_say", { line });
}

/** Settings → Pulse → Interests (src-tauri/src/interests.rs). */
export interface InterestTopic {
  topic: string;
  pinned: boolean;
  muted: boolean;
  /** "From memory", "From chats", "From saved" or "Added by you". */
  source: string;
}

export interface InterestPage {
  /** Pinned, then by weight, then muted. */
  topics: InterestTopic[];
  note: string;
}

export type InterestAct =
  | { kind: "pin"; on: boolean }
  | { kind: "mute"; on: boolean }
  | { kind: "remove" };

export function getInterests(): Promise<InterestPage> {
  return invoke<InterestPage>("get_interests");
}

export function addInterest(text: string): Promise<InterestPage> {
  return invoke<InterestPage>("add_interest", { text });
}

export function interestAct(
  topic: string,
  act: InterestAct,
): Promise<InterestPage> {
  return invoke<InterestPage>("interest_act", { topic, act });
}

/** Settings → Permissions (src-tauri/src/permissions.rs). */
export type RuleKind = "allow" | "ask" | "deny";

export interface PermissionRule {
  action: RuleKind;
  /** As typed, like Bash(git *). */
  rule: string;
}

export interface PermissionPage {
  /** Saved for every project, in order. */
  rules: PermissionRule[];
  /** From this project's .claude/settings.json; read only. */
  imported: PermissionRule[];
  /** Commands remembered with Allow always for this project. */
  grants: string[];
}

export function getPermissions(): Promise<PermissionPage> {
  return invoke<PermissionPage>("get_permissions");
}

export function addPermissionRule(
  rule: string,
  kind: RuleKind,
): Promise<PermissionPage> {
  return invoke<PermissionPage>("add_permission_rule", { rule, kind });
}

export function removePermissionRule(
  index: number,
  rule: string,
): Promise<PermissionPage> {
  return invoke<PermissionPage>("remove_permission_rule", { index, rule });
}

export function rememberGrant(command: string): Promise<PermissionPage> {
  return invoke<PermissionPage>("remember_grant", { command });
}

export function forgetGrant(command: string): Promise<PermissionPage> {
  return invoke<PermissionPage>("forget_grant", { command });
}

/** Settings → About → Updates (src-tauri/src/shell_update.rs). */
export type UpdateChannel = "stable" | "beta";

export interface UpdateView {
  version: string;
  channel: UpdateChannel;
  /** The AppImage and the Windows installer update themselves. */
  updatable: boolean;
  /** Why this install doesn't update itself. */
  note: string | null;
  /** The version a check found, ready to install. */
  available: string | null;
}

/** Emitted when the background check (every 2 hours) finds an update. */
export const UPDATE_EVENT = "update://available";

export function getUpdateStatus(): Promise<UpdateView> {
  return invoke<UpdateView>("get_update_status");
}

export function setUpdateChannel(channel: UpdateChannel): Promise<UpdateView> {
  return invoke<UpdateView>("set_update_channel", { channel });
}

export function checkShellUpdate(): Promise<UpdateView> {
  return invoke<UpdateView>("check_shell_update");
}

/** Downloads, verifies and installs the update, then restarts into it. */
export function installShellUpdate(): Promise<void> {
  return invoke<void>("install_shell_update");
}

export function onUpdateAvailable(
  handler: (view: UpdateView) => void,
): Promise<UnlistenFn> {
  return subscribe<UpdateView>(UPDATE_EVENT, handler);
}

/** The Skills page (src-tauri/src/skills.rs). Built-in skills stay off it. */
export type SkillSource = "cabin" | "project" | "user" | "plugin" | "builtin";

export interface SkillRow {
  name: string;
  description: string;
  /** What Use puts in the chat box. */
  trigger: string;
  source: SkillSource;
  /** The skill's folder, empty for a built-in. */
  folder: string;
  /** False when it's switched off: it stays listed but chat won't use it. */
  on: boolean;
}

export interface SkillPage {
  rows: SkillRow[];
}

export function getSkills(): Promise<SkillPage> {
  return invoke<SkillPage>("get_skills");
}

/** Switch a skill on or off for chat. Desktop only; returns the new page. */
export function setSkillOn(name: string, on: boolean): Promise<SkillPage> {
  return invoke<SkillPage>("set_skill_on", { name, on });
}

/** The shell finds the folder from the source and name itself. */
export function openSkillFolder(
  source: SkillSource,
  name: string,
): Promise<void> {
  return invoke<void>("open_skill_folder", { source, name });
}

/** A background task's status on the Agents page (src-tauri/src/queue.rs). */
export type TaskStatus = "running" | "failed" | "done" | "stopped";

/** A background command, subagent or monitor of the open chat. */
export interface QueueTask {
  id: string;
  title: string;
  status: TaskStatus;
  /** Still going, so Stop has something to stop. */
  live: boolean;
}

/** A to-do Grok wrote in the chat: "pending", "in_progress", "completed" or "cancelled". */
export interface QueueTodo {
  id: string;
  content: string;
  status: string;
}

export interface Queue {
  todos: QueueTodo[];
  tasks: QueueTask[];
}

export function getQueue(conversationId: string): Promise<Queue> {
  return invoke<Queue>("get_queue", { conversationId });
}

/** Stops one task, or every task of the chat when `taskId` is left out. */
export function stopTask(
  conversationId: string,
  taskId?: string,
): Promise<Queue> {
  return invoke<Queue>("stop_task", { conversationId, taskId: taskId ?? null });
}

/** Settings → Behavior, saved in shell.json. Everything starts off. */
export interface Behavior {
  autocomplete: boolean;
  /** A few next steps to tap, under the box, on Home and after replies. */
  suggestions: boolean;
  /** Quiet hours for automations as HH:MM; both empty is none. */
  quietStart: string;
  quietEnd: string;
  /** Tokens a day, one of `budgets`; 0 is off. */
  dailyTokenBudget: number;
  pauseOverBudget: boolean;
}

export interface BehaviorPage {
  closeToTray: boolean;
  behavior: Behavior;
  budgets: { value: number; label: string }[];
}

export function getBehavior(): Promise<BehaviorPage> {
  return invoke<BehaviorPage>("get_behavior");
}

export function saveBehavior(
  closeToTray: boolean,
  behavior: Behavior,
): Promise<BehaviorPage> {
  return invoke<BehaviorPage>("save_behavior", { closeToTray, behavior });
}

/** Settings → Your PC → PC helpers (src-tauri/src/pc_helpers.rs). */
export interface PcHelpers {
  focusHelp: boolean;
  windDown: boolean;
  /** The saved idle time in minutes, once one is picked. */
  autoSleepMin: number | null;
  sleepChoices: { minutes: number; label: string }[];
  /** The last read couldn't see a game, fullscreen app or GPU work. */
  sleepBlind: boolean;
  rhythm: boolean;
  rhythmGames: boolean;
  rhythmMinutes: number;
  rhythmSummary: string;
}

export type PcHelperKey = "focusHelp" | "windDown" | "rhythm" | "rhythmGames";

export function getPcHelpers(): Promise<PcHelpers> {
  return invoke<PcHelpers>("get_pc_helpers");
}

export function setPcHelper(key: PcHelperKey, on: boolean): Promise<PcHelpers> {
  return invoke<PcHelpers>("set_pc_helper", { key, on });
}

export function setPcAutoSleep(minutes: number): Promise<PcHelpers> {
  return invoke<PcHelpers>("set_pc_auto_sleep", { minutes });
}

export function forgetPcRhythm(): Promise<PcHelpers> {
  return invoke<PcHelpers>("forget_pc_rhythm");
}

/** Settings → Defaults: the work folder and the approvals. */
export function saveDefaults(
  workspace: string,
  approvals: Approvals,
): Promise<Settings> {
  return invoke<Settings>("save_defaults", { workspace, approvals });
}

/** Settings → Privacy & data (src-tauri/src/privacy.rs). */
export interface PrivacyPage {
  offlineOnly: boolean;
  /** An on-device model can take the chat while Offline only is on. */
  localReady: boolean;
  /** "3 calls sent off-device this week". */
  sent: string;
  hosts: { host: string; calls: number }[];
  encrypted: boolean;
  encryptedNote: string;
  dataFolder: string;
  /** Where exports and the cabin's other files go (Documents/Loadout). */
  filesFolder: string;
  canOpenFolder: boolean;
}

export function getPrivacy(): Promise<PrivacyPage> {
  return invoke<PrivacyPage>("get_privacy");
}

export function setOfflineOnly(on: boolean): Promise<PrivacyPage> {
  return invoke<PrivacyPage>("set_offline_only", { on });
}

export function openDataFolder(): Promise<void> {
  return invoke<void>("open_data_folder");
}

export function openFilesFolder(): Promise<void> {
  return invoke<void>("open_files_folder");
}

/** The folder picker; the page as it is after (unchanged when closed). */
export function changeFilesFolder(): Promise<PrivacyPage> {
  return invoke<PrivacyPage>("change_files_folder");
}

/** Settings → Background (src-tauri/src/background.rs). */
export interface BackgroundPage {
  runner: string;
  nextDue: string | null;
  quiet: string;
  budget: string;
  rows: {
    id: string;
    name: string;
    next: string;
    last: string;
    health: string | null;
  }[];
}

export function getBackground(): Promise<BackgroundPage> {
  return invoke<BackgroundPage>("get_background");
}

/** One Steam game on this computer (src-tauri/src/library.rs). */
export interface LibraryGame {
  appId: number;
  name: string;
  /** False while Steam downloads or updates it. */
  installed: boolean;
  /** Unix seconds, 0 when never played. */
  lastPlayed: number;
  /** Minutes played, 0 when never played. */
  playtimeMin: number;
}

export interface LibraryPage {
  /** A Steam library folder was found on this computer. */
  steamFound: boolean;
  /** Most recently played first. */
  games: LibraryGame[];
}

/** Tall box art for the grid, the wide store header for the detail column. */
export type CoverShape = "tall" | "wide";

export function getLibrary(): Promise<LibraryPage> {
  return invoke<LibraryPage>("get_library");
}

/** A cover as a `data:` URL, or null when Steam has none (paint a placeholder). */
export function libraryCover(
  appId: number,
  shape: CoverShape,
): Promise<string | null> {
  return invoke<string | null>("library_cover", { appId, shape });
}

/** Ask Steam to start the game. Resolves to a line to show. */
export function libraryPlay(appId: number): Promise<string> {
  return invoke<string>("library_play", { appId });
}

/** One Battle.net game tile (src-tauri/src/battlenet.rs). */
export interface BattlenetGame {
  product: string;
  name: string;
  install: string;
  prefix?: string | null;
  /** Painted cover colour seed. */
  seed: number;
}

export interface BattlenetPage {
  found: boolean;
  games: BattlenetGame[];
}

export function getBattlenet(): Promise<BattlenetPage> {
  return invoke<BattlenetPage>("get_battlenet");
}

/** Ask Battle.net to start the product. */
export function battlenetPlay(product: string): Promise<string> {
  return invoke<string>("battlenet_play", { product });
}

/** Open the install folder for a Battle.net product. */
export function battlenetOpenFolder(product: string): Promise<string> {
  return invoke<string>("battlenet_open_folder", { product });
}

/** Per-game companion frame: note, news since last play, guides link. */
export interface CompanionNews {
  title: string;
  url: string;
}

export interface CompanionFrame {
  note: string;
  news: CompanionNews[];
  guidesUrl: string | null;
}

export function companionFrame(
  noteId: number,
  lastPlayed: number,
  steamAppId?: number | null,
): Promise<CompanionFrame> {
  return invoke<CompanionFrame>("companion_frame", {
    noteId,
    lastPlayed,
    steamAppId: steamAppId ?? null,
  });
}

export function companionSetNote(appId: number, text: string): Promise<string> {
  return invoke<string>("companion_set_note", { appId, text });
}

/** Per-game companion screens (src-tauri/src/games.rs). */
export interface WowFlavorRow {
  install: string;
  flavor: string;
  label: string;
  addonLine: string;
  action: string | null;
  characters: string[];
  events: string[];
  emptySaved: string | null;
}

export interface WowView {
  title: string;
  running: boolean;
  caption: string;
  logging: string | null;
  watcherNote: string | null;
  flavors: WowFlavorRow[];
  installAll: string | null;
  note: string;
}

export interface Gw2View {
  status: string;
  hasKey: boolean;
  accountLine: string | null;
  note: string;
  keyHint: string;
}

export interface Gw2KeyCard {
  id: string;
  action: string;
}

export interface EsoServerRow {
  dir: string;
  label: string;
  status: string;
  action: string | null;
}

export interface EsoView {
  servers: EsoServerRow[];
  character: string | null;
  note: string;
  empty: string | null;
}

export interface OsrsView {
  status: string;
  gains: string | null;
  hasPlayer: boolean;
  name: string | null;
  kind: string;
  kinds: string[];
  note: string;
  nameHint: string;
}

export function gameWow(): Promise<WowView> {
  return invoke<WowView>("game_wow");
}

export function gameWowAddon(
  install: string,
  flavor: string,
  action: string,
): Promise<WowView> {
  return invoke<WowView>("game_wow_addon", { install, flavor, action });
}

export function gameGw2(): Promise<Gw2View> {
  return invoke<Gw2View>("game_gw2");
}

export function gameGw2Refresh(): Promise<Gw2View> {
  return invoke<Gw2View>("game_gw2_refresh");
}

export function gameGw2StartKey(key: string): Promise<Gw2KeyCard> {
  return invoke<Gw2KeyCard>("game_gw2_start_key", { key });
}

export function gameGw2AnswerKey(id: string, approve: boolean): Promise<Gw2View> {
  return invoke<Gw2View>("game_gw2_answer_key", { id, approve });
}

export function gameGw2RemoveKey(): Promise<Gw2View> {
  return invoke<Gw2View>("game_gw2_remove_key");
}

export function gameEso(): Promise<EsoView> {
  return invoke<EsoView>("game_eso");
}

export function gameEsoAddon(dir: string, install: boolean): Promise<EsoView> {
  return invoke<EsoView>("game_eso_addon", { dir, install });
}

export function gameOsrs(): Promise<OsrsView> {
  return invoke<OsrsView>("game_osrs");
}

export function gameOsrsSave(name: string, kind: string): Promise<OsrsView> {
  return invoke<OsrsView>("game_osrs_save", { name, kind });
}

export function gameOsrsForget(): Promise<OsrsView> {
  return invoke<OsrsView>("game_osrs_forget");
}

export function gameOsrsRefresh(): Promise<OsrsView> {
  return invoke<OsrsView>("game_osrs_refresh");
}

export interface DestinyView {
  status: string;
  hasKey: boolean;
  hasName: boolean;
  bungieName: string | null;
  profileLine: string | null;
  note: string;
  howTo: string;
  keyHint: string;
  nameHint: string;
}

export interface DestinyKeyCard {
  id: string;
  action: string;
}

export interface PoeView {
  title: string;
  appId: number;
  status: string;
  empty: string | null;
  note: string;
}

export interface EliteView {
  title: string;
  status: string;
  empty: string | null;
  note: string;
}

export interface FactorioView {
  status: string;
  warning: string | null;
  action: string | null;
  note: string;
  empty: string | null;
  userDir: string | null;
}

export interface StardewView {
  title: string;
  status: string;
  empty: string | null;
  note: string;
}

export interface MinecraftView {
  title: string;
  status: string;
  empty: string | null;
  note: string;
}

export interface RocketLeagueView {
  title: string;
  status: string;
  howTo: boolean;
  iniLine: string;
  note: string;
}

export function gameDestiny(): Promise<DestinyView> {
  return invoke<DestinyView>("game_destiny");
}

export function gameDestinyRefresh(): Promise<DestinyView> {
  return invoke<DestinyView>("game_destiny_refresh");
}

export function gameDestinyStartKey(key: string): Promise<DestinyKeyCard> {
  return invoke<DestinyKeyCard>("game_destiny_start_key", { key });
}

export function gameDestinyAnswerKey(id: string, approve: boolean): Promise<DestinyView> {
  return invoke<DestinyView>("game_destiny_answer_key", { id, approve });
}

export function gameDestinySaveName(name: string): Promise<DestinyView> {
  return invoke<DestinyView>("game_destiny_save_name", { name });
}

export function gameDestinyRemove(): Promise<DestinyView> {
  return invoke<DestinyView>("game_destiny_remove");
}

export function gamePoe(appId: number): Promise<PoeView> {
  return invoke<PoeView>("game_poe", { appId });
}

export function gameElite(): Promise<EliteView> {
  return invoke<EliteView>("game_elite");
}

export function gameFactorio(): Promise<FactorioView> {
  return invoke<FactorioView>("game_factorio");
}

export function gameFactorioMod(dir: string, install: boolean): Promise<FactorioView> {
  return invoke<FactorioView>("game_factorio_mod", { dir, install });
}

export function gameStardew(): Promise<StardewView> {
  return invoke<StardewView>("game_stardew");
}

export function gameMinecraft(): Promise<MinecraftView> {
  return invoke<MinecraftView>("game_minecraft");
}

export function gameRl(): Promise<RocketLeagueView> {
  return invoke<RocketLeagueView>("game_rl");
}

/** Settings → Behavior / Library companion: game watcher (`app.json` gameWatch). */
export interface GameWatch {
  on: boolean;
  note: string;
}

export function getGameWatch(): Promise<GameWatch> {
  return invoke<GameWatch>("get_game_watch");
}

export function setGameWatch(on: boolean): Promise<GameWatch> {
  return invoke<GameWatch>("set_game_watch", { on });
}

/** Settings → Pulse: Steam achievement nudges and wishlist sale alerts. */
export interface SteamPulseThreshold {
  key: string;
  label: string;
}

export interface SteamPulse {
  achievementsOn: boolean;
  wishlistOn: boolean;
  threshold: string;
  thresholds: SteamPulseThreshold[];
  /** Sealed SteamID on disk — never the digits. */
  hasId: boolean;
  canUseLocal: boolean;
  achNote: string;
  wishNote: string;
  achPrivate: boolean;
  wishPrivate: boolean;
}

export function getSteamPulse(): Promise<SteamPulse> {
  return invoke<SteamPulse>("get_steam_pulse");
}

export function setSteamAchievements(on: boolean): Promise<SteamPulse> {
  return invoke<SteamPulse>("set_steam_achievements", { on });
}

export function setSteamWishlist(on: boolean): Promise<SteamPulse> {
  return invoke<SteamPulse>("set_steam_wishlist", { on });
}

export function setSteamWishlistThreshold(threshold: string): Promise<SteamPulse> {
  return invoke<SteamPulse>("set_steam_wishlist_threshold", { threshold });
}

export function saveSteamId(draft: string, useLocal: boolean): Promise<SteamPulse> {
  return invoke<SteamPulse>("save_steam_id", { draft, useLocal });
}

export function forgetSteamId(): Promise<SteamPulse> {
  return invoke<SteamPulse>("forget_steam_id");
}

export function steamCheckNow(module: "achievements" | "deals"): Promise<SteamPulse> {
  return invoke<SteamPulse>("steam_check_now", { module });
}

/** A Home tile's Turn on, for a switch that needs nothing else. */
export function homeTurnOn(feature: string): Promise<void> {
  return invoke<void>("home_turn_on", { feature });
}

/** A tile's Set up (or Undo setup) behind the OS's own password or admin
 * prompt; answers with one plain sentence on how it went. */
export function homeSetup(module: string, undo: boolean): Promise<string> {
  return invoke<string>("home_setup", { module, undo });
}

/** A tile's setup steps, to paste into a terminal by hand. */
export function homeSetupSteps(module: string): Promise<string> {
  return invoke<string>("home_setup_steps", { module });
}

/** Opens Steam's own friends list, for Home's Squad tile. */
export function libraryFriends(): Promise<void> {
  return invoke<void>("library_friends");
}

/** Opens Steam's Settings (Game Recording), for Home's Clips tile. */
export function librarySteamSettings(): Promise<void> {
  return invoke<void>("library_steam_settings");
}

/** Starts a playbook on a new Workboard card, for Home's Playbooks tile;
 * answers with the ask for the chat box. */
export function homeRunPlaybook(id: string): Promise<BoardWork> {
  return invoke<BoardWork>("home_run_playbook", { id });
}

/** Plays the clip at this place in Home's Clips tile with the PC's player. */
export function homeOpenClip(index: number): Promise<void> {
  return invoke<void>("home_open_clip", { index });
}

/** Home's Updates & Drivers page (src-tauri/src/home_updates.rs, core home::updates). */
export type UpdateOffer =
  | "updateThisApp"
  | "openSteamDownloads"
  | "updateSystem"
  | "updateApps"
  | "openWindowsUpdate"
  | "updateDrivers"
  | "rollBackDrivers"
  | "openOptionalUpdates";

export interface UpdateItem {
  name: string;
  from: string | null;
  to: string | null;
}

export interface UpdateGroup {
  kind: "loadout" | "steam" | "system" | "apps" | "windows" | "drivers";
  title: string;
  line: string;
  items: UpdateItem[];
  offer: { offer: UpdateOffer; label: string; held: string | null } | null;
}

export interface UpdatesPage {
  headline: string;
  groups: UpdateGroup[];
  driver: string | null;
}

/** Reads what waits to update on this PC (lists already on disk; no refresh). */
export function getUpdatesPage(): Promise<UpdatesPage> {
  return invoke<UpdatesPage>("updates_page");
}

/** Runs one offer: opens its page, or updates behind the OS prompt. Returns a sentence. */
export function runUpdateOffer(offer: UpdateOffer): Promise<string> {
  return invoke<string>("run_update_offer", { offer });
}

/** Code tab: one changed file in the workspace's repo. */
export interface CodeChanged {
  path: string;
  status:
    | "modified"
    | "added"
    | "deleted"
    | "renamed"
    | "untracked"
    | "conflict"
    | "changed";
  staged: boolean;
}

/** Code tab: the workspace's repo, read only. */
export interface CodeRepo {
  workspace: string;
  isRepo: boolean;
  branch: string | null;
  upstream: string | null;
  ahead: number;
  behind: number;
  files: CodeChanged[];
  more: boolean;
}

export interface CodeDiff {
  path: string;
  text: string;
  cut: boolean;
}

/** Code tab: one project on the project bar. */
export interface CodeProject {
  id: string;
  name: string;
  path: string;
  /** The folder the app works in now. */
  current: boolean;
  /** The folder is no longer on disk. */
  missing: boolean;
}

/** Code tab: every project in the list, and the open folder ("" when none is). */
export interface CodeProjects {
  bound: string;
  projects: CodeProject[];
}

export function codeProjects(): Promise<CodeProjects> {
  return invoke<CodeProjects>("code_projects");
}

/** Switch to a project from the list, or open a folder by path; the app then works in it. */
export function codeProjectOpen(path: string): Promise<CodeProjects> {
  return invoke<CodeProjects>("code_project_open", { path });
}

/** The system folder picker, then that folder opens; null when the picker was closed. */
export function codeProjectPick(): Promise<CodeProjects | null> {
  return invoke<CodeProjects | null>("code_project_pick");
}

/** Make a project in the work folder and open it. */
export function codeProjectNew(name: string): Promise<CodeProjects> {
  return invoke<CodeProjects>("code_project_new", { name });
}

/** The agent window shows this session: it takes the open project's folder
 * unless it has one already. Resolves with the folder it works in. */
export function codeSessionBind(conversationId: string): Promise<string> {
  return invoke<string>("code_session_bind", { conversationId });
}

export function codeRepo(): Promise<CodeRepo> {
  return invoke<CodeRepo>("code_repo");
}

export function codeDiff(path: string): Promise<CodeDiff> {
  return invoke<CodeDiff>("code_diff", { path });
}

/** A same-name project, user, cabin or plugin skill that shadows a built-in. */
export interface CodeOverriddenBy {
  scope: "project" | "user" | "cabin" | "plugin" | string;
  name: string;
}

/** Code tab: one built-in skill, always listed, even when overridden. */
export interface CodeBuiltin {
  name: string;
  description: string;
  body: string;
  overriddenBy: CodeOverriddenBy | null;
}

export function codeBuiltins(): Promise<CodeBuiltin[]> {
  return invoke<CodeBuiltin[]>("code_builtins");
}

/** Code tab: the test command to show, and whether the first run still needs a confirm. */
export interface CodeTestPlan {
  command: string;
  needsConfirm: boolean;
  remembered: string | null;
}

/** A finished test run. Output is already redacted. */
export interface CodeTestResult {
  command: string;
  exit: number;
  output: string;
  cut: boolean;
}

/** Settings → Code: the remembered test command for this project, if any. */
export interface CodeTestRemembered {
  command: string | null;
}

/** A redacted chunk of test output, or the end of the run. */
export interface CodeRunChunk {
  text: string;
  done: boolean;
  exit: number | null;
  cut: boolean;
}

export function codeTestPlan(command?: string | null): Promise<CodeTestPlan> {
  return invoke<CodeTestPlan>("code_test_plan", { command: command ?? null });
}

export function codeTestRun(
  command: string,
  confirmed: boolean,
): Promise<CodeTestResult> {
  return invoke<CodeTestResult>("code_test_run", { command, confirmed });
}

export function codeTestRemembered(): Promise<CodeTestRemembered> {
  return invoke<CodeTestRemembered>("code_test_remembered");
}

export function codeTestForget(): Promise<CodeTestRemembered> {
  return invoke<CodeTestRemembered>("code_test_forget");
}

/** The Command page: the project folder's name and the lines it remembers. */
export interface CommandBoxView {
  project: string;
  remembered: string[];
}

/** What the Command page does with a line: queue it to the agent now, ask
 * first (a first run, or a destructive line every time), or refuse it. */
export type CommandBoxVerdict =
  | { verdict: "queue"; command: string; prompt: string }
  | { verdict: "confirm"; command: string; destructive: boolean }
  | { verdict: "blocked"; command: string; message: string };

export function commandBoxView(): Promise<CommandBoxView> {
  return invoke<CommandBoxView>("command_box_view");
}

export function commandBoxCheck(line: string): Promise<CommandBoxVerdict> {
  return invoke<CommandBoxVerdict>("command_box_check", { line });
}

/** The yes on a first run: the checks run again, and the agent's message comes back. */
export function commandBoxApprove(line: string): Promise<string> {
  return invoke<string>("command_box_approve", { line });
}

export function commandBoxForget(): Promise<CommandBoxView> {
  return invoke<CommandBoxView>("command_box_forget");
}

export function onCodeRun(
  handler: (chunk: CodeRunChunk) => void,
): Promise<UnlistenFn> {
  return subscribe<CodeRunChunk>("code-run", handler);
}

/** Stop the running test or deploy and everything it started (desktop app
 * only). Resolves to false when nothing was running. */
export function codeRunStop(): Promise<boolean> {
  return invoke<boolean>("code_run_stop");
}

/** Code tab editor: one entry of a project folder. */
export interface CodeFileEntry {
  name: string;
  /** From the project root, with `/` between parts. */
  path: string;
  dir: boolean;
}

export interface CodeFileListing {
  /** The folder, from the project root (`""` for the root). */
  path: string;
  entries: CodeFileEntry[];
  more: boolean;
}

export interface CodeFileText {
  path: string;
  text: string;
  /** The project folder the file was read from; a save sends it back. */
  root: string;
}

/** One folder of the project (desktop app only). */
export function codeFilesList(path: string): Promise<CodeFileListing> {
  return invoke<CodeFileListing>("code_files_list", { path });
}

/** A text file of the project, for the editor (desktop app only). */
export function codeFileRead(path: string): Promise<CodeFileText> {
  return invoke<CodeFileText>("code_file_read", { path });
}

/** Save a text file of the project; the save goes in the activity log.
 * `root` is the project folder the file was opened from: the desktop refuses
 * the save when that is no longer the project folder. */
export function codeFileWrite(path: string, text: string, root: string): Promise<CodeFileText> {
  return invoke<CodeFileText>("code_file_write", { path, text, root });
}

/** Redacted terminal output, or the news that the shell ended. */
export interface CodeTermChunk {
  id: number;
  text: string;
  exited: boolean;
}

/** Start the user's shell in the project folder; resolves to the session id. */
export function codeTermOpen(rows: number, cols: number): Promise<number> {
  return invoke<number>("code_term_open", { rows, cols });
}

export function codeTermWrite(id: number, data: string): Promise<void> {
  return invoke<void>("code_term_write", { id, data });
}

export function codeTermResize(
  id: number,
  rows: number,
  cols: number,
): Promise<void> {
  return invoke<void>("code_term_resize", { id, rows, cols });
}

export function codeTermClose(): Promise<void> {
  return invoke<void>("code_term_close");
}

export function onCodeTerm(
  handler: (chunk: CodeTermChunk) => void,
): Promise<UnlistenFn> {
  return subscribe<CodeTermChunk>("code-term", handler);
}

/** Settings → Code → Pushes and PRs (`shell.json` → `codeShip`). */
export type CodeShipMode = "ask" | "autoBeta";
export interface CodeShipSettings {
  mode: CodeShipMode;
  /** The only PR base Auto-beta works with, and the base of "Open PR into". */
  autoBase: string;
  hosts: { github: boolean; other: boolean };
  /** Protected on top of main and the repo's default branch. */
  extraProtected: string[];
  activityKeep: number;
}

export type CodeShipAction = "push" | "pr";

/** What a push or "Open PR into" would run, and whether it asks first. */
export interface CodeShipPlan {
  action: CodeShipAction;
  branch: string | null;
  command: string;
  needsConfirm: boolean;
  blocked: string | null;
  mode: CodeShipMode;
  autoBase: string;
  appMade: boolean;
  prUrl: string | null;
  /** A push only: the repo has a pre-push hook, and this push does not run it. */
  skipsHook: boolean;
}

/** A finished push or PR. Output is already redacted. */
export interface CodeShipped {
  command: string;
  exit: number;
  output: string;
  summary: string;
}

/** One line of the Code tab's activity log. */
export interface CodeActivityEntry {
  tsMs: number;
  kind:
    | "push"
    | "pr"
    | "branch"
    | "blocked"
    | "deploy"
    | "secret"
    | "edit"
    | "terminal";
  repoKey: string;
  host: string;
  branch: string;
  command: string;
  mode: string;
  approvedBy: "user" | "autoBeta" | "remembered" | "none";
  exit: number | null;
  summary: string;
}

export function codeShipPlan(action: CodeShipAction): Promise<CodeShipPlan> {
  return invoke<CodeShipPlan>("code_ship_plan", { action });
}

/** `command` is the command the confirm card showed; the app refuses when it would now run a different one. */
export function codeShipRun(
  action: CodeShipAction,
  confirmed: boolean,
  command?: string | null,
): Promise<CodeShipped> {
  return invoke<CodeShipped>("code_ship_run", {
    action,
    confirmed,
    command: command ?? null,
  });
}

export function codeNewBranch(name: string): Promise<string> {
  return invoke<string>("code_new_branch", { name });
}

export function codeActivity(): Promise<CodeActivityEntry[]> {
  return invoke<CodeActivityEntry[]>("code_activity_list");
}

export function getCodeShip(): Promise<CodeShipSettings> {
  return invoke<CodeShipSettings>("get_code_ship");
}

export function saveCodeShip(
  ship: CodeShipSettings,
): Promise<CodeShipSettings> {
  return invoke<CodeShipSettings>("save_code_ship", { ship });
}

/** Settings → Code → Secrets: this project's secret names. Values never come back to the window. */
export interface CodeSecretList {
  names: string[];
}

export function codeSecretList(): Promise<CodeSecretList> {
  return invoke<CodeSecretList>("code_secret_list");
}

/** Add a secret or replace its value. The value goes to the system keychain. */
export function codeSecretSet(
  name: string,
  value: string,
): Promise<CodeSecretList> {
  return invoke<CodeSecretList>("code_secret_set", { name, value });
}

/** Delete a secret. The shell refuses unless `confirmed` is true. */
export function codeSecretDelete(
  name: string,
  confirmed: boolean,
): Promise<CodeSecretList> {
  return invoke<CodeSecretList>("code_secret_delete", { name, confirmed });
}

/** Code tab and Settings → Code: this project's deploy command and whether it asks first. */
export interface CodeDeployPlan {
  deployEnabled: boolean;
  command: string;
  needsConfirm: boolean;
  /** The command currently remembered for this project, if any. */
  remembered: string | null;
  mode: CodeShipMode;
  /** Why a deploy can't run now, if it can't. */
  blocked: string | null;
}

/** One project's deploy settings. Off until turned on. */
export interface CodeDeploySettings {
  deployEnabled: boolean;
  command: string;
}

export function codeDeployPlan(): Promise<CodeDeployPlan> {
  return invoke<CodeDeployPlan>("code_deploy_plan");
}

export function codeDeploySave(
  deployEnabled: boolean,
  command: string,
): Promise<CodeDeploySettings> {
  return invoke<CodeDeploySettings>("code_deploy_save", {
    deployEnabled,
    command,
  });
}

export function codeDeployForget(): Promise<CodeDeployPlan> {
  return invoke<CodeDeployPlan>("code_deploy_forget");
}

/** Run the deploy. `command` is the text the person was shown; the shell refuses if it changed. */
export function codeDeployRun(
  command: string,
  confirmed: boolean,
): Promise<CodeTestResult> {
  return invoke<CodeTestResult>("code_deploy_run", { command, confirmed });
}

export function onCodeDeployRun(
  handler: (chunk: CodeRunChunk) => void,
): Promise<UnlistenFn> {
  return subscribe<CodeRunChunk>("code-deploy-run", handler);
}

/** One other AI provider you added. The key itself never reaches the page. */
export interface AiProviderRow {
  id: string;
  kind: string;
  baseUrl: string;
  dest: string;
  hasKey: boolean;
  /** "No key", "Not allowed yet" or "Allowed". */
  state: string;
  ready: boolean;
}

export interface AiSpend {
  weeklyCapUsd: number;
  priceCeilingUsdPerM: number;
  caps: number[];
  ceilings: number[];
}

export interface AiPremiumRow {
  id: string;
  label: string;
}

export interface AiTableRow {
  model: string;
  effort: string | null;
  quality: number | null;
  estCostUsd: number | null;
  p50LatencyMs: number | null;
  why: string;
}

export interface AiClassView {
  class: string;
  rows: AiTableRow[];
}

export interface AiModelsView {
  providers: AiProviderRow[];
  spend: AiSpend;
  premium: AiPremiumRow[];
  table: AiClassView[];
  tableBuiltAt: number;
  locked: string;
  status: string;
}

/** A card waiting for Save/Allow or Deny. It never holds a key. */
export interface AiModelsCard {
  id: string;
  kind: "key" | "allow";
  provider: string;
  action: string;
}

export function getAiModels(): Promise<AiModelsView> {
  return invoke<AiModelsView>("get_ai_models");
}

/** Posts the key's approval card; nothing is saved until `answerAiModelsCard`. */
export function addAiProvider(
  baseUrl: string,
  key: string,
): Promise<AiModelsCard> {
  return invoke<AiModelsCard>("add_ai_provider", { baseUrl, key });
}

export function allowAiProvider(id: string): Promise<AiModelsCard> {
  return invoke<AiModelsCard>("allow_ai_provider", { id });
}

export function answerAiModelsCard(
  id: string,
  approve: boolean,
): Promise<AiModelsView> {
  return invoke<AiModelsView>("answer_ai_models_card", { id, approve });
}

export function removeAiProvider(id: string): Promise<AiModelsView> {
  return invoke<AiModelsView>("remove_ai_provider", { id });
}

export function saveAiSpend(
  cap: number,
  ceiling: number,
): Promise<AiModelsView> {
  return invoke<AiModelsView>("save_ai_spend", { cap, ceiling });
}

export function revokeAiPremium(id: string): Promise<AiModelsView> {
  return invoke<AiModelsView>("revoke_ai_premium", { id });
}

/** One line of the top bar while a recording or an audio check runs. */
export interface RecordLine {
  secs: number;
  text: string;
}

/** A run that just finished (src-tauri/src/screen_record.rs). */
export interface RecordEvent {
  kind: "record" | "audio";
  ok: boolean;
  text: string;
}

export interface RecordStatus {
  recording: RecordLine | null;
  listening: RecordLine | null;
  events: RecordEvent[];
}

/** The recording indicator, the audio check's line and finished runs (each once). */
export function recordStatus(): Promise<RecordStatus> {
  return invoke<RecordStatus>("record_status");
}

/** The indicator's Stop. */
export function recordStop(): Promise<boolean> {
  return invoke<boolean>("record_stop");
}

/** One source a game can be modded from (src-tauri/src/mods.rs). */
export interface ModSource {
  id: string;
  label: string;
}

/** A game Loadout may mod. */
export interface ModGame {
  id: string;
  name: string;
  /** What Loadout mods there ("Singleplayer only" and the like). */
  modeNote: string;
  /** Preferred first. */
  sources: ModSource[];
  /** Unix seconds, 0 when never played. */
  lastPlayed: number;
  playtimeMin: number;
  /** New finds not opened yet. */
  newCount: number;
  /** For you leaves this game out. */
  suggestionsHidden: boolean;
}

/** A game Loadout won't mod, and why. */
export interface CantModGame {
  key: string;
  name: string;
  reason: string;
}

export interface ModGamesView {
  /** Most recently played first. */
  games: ModGame[];
  /** Shown collapsed under "Can't mod". */
  cant: CantModGame[];
  /** What discovery is doing, one quiet line. */
  status: string;
  offline: boolean;
}

/** One mod in a list. */
export interface ModRow {
  id: string;
  name: string;
  author: string;
  summary: string;
  downloads: number;
  likes: number;
  /** Unix seconds, 0 when the source didn't say. */
  updated: number;
  /** The mod's page on the web. */
  pageUrl: string;
  iconUrl: string | null;
  categories: string[];
}

export interface ModPage {
  mods: ModRow[];
  page: number;
  more: boolean;
  /** A plain line when the source failed or Offline only is on. */
  note: string | null;
  /** Shown from the cache rather than asked just now. */
  cached: boolean;
}

/** The lists a game's page can show. */
export type ModList = "popular" | "trending" | "updated";

/** One For you card. */
export interface ModSuggestion {
  game: string;
  gameName: string;
  source: string;
  /** The tab it opens on. */
  tab: "newFinds" | "trending" | "popular";
  /** Why it shows, in a few words. */
  why: string;
  mod: ModRow;
}

/** The Mods rail; also starts discovery when it is due. */
export function modsGames(): Promise<ModGamesView> {
  return invoke<ModGamesView>("mods_games");
}

export function modsBrowse(game: string, source: string, list: ModList, page: number): Promise<ModPage> {
  return invoke<ModPage>("mods_browse", { game, source, list, page });
}

export function modsSearch(game: string, source: string, text: string, page: number): Promise<ModPage> {
  return invoke<ModPage>("mods_search", { game, source, text, page });
}

/** A game's New finds; opening them marks them seen. */
export function modsNewFinds(game: string, source: string, page: number): Promise<ModPage> {
  return invoke<ModPage>("mods_new_finds", { game, source, page });
}

export function modsForYou(): Promise<ModSuggestion[]> {
  return invoke<ModSuggestion[]>("mods_for_you");
}

/** That mod never shows in For you again. */
export function modsNotForMe(source: string, id: string): Promise<void> {
  return invoke<void>("mods_not_for_me", { source, id });
}

/** Hide or show For you's suggestions for one game. */
export function modsHideGame(game: string, hidden: boolean): Promise<void> {
  return invoke<void>("mods_hide_game", { game, hidden });
}

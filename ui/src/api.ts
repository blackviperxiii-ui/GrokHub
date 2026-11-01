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
  model: string;
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
  /** The web app, a remote for the desktop: settings and the key stay there. */
  remote?: boolean;
}

/** Until the report loads, or if it fails, only the core features show. */
export const CORE_ONLY: Capabilities = {
  desktopControl: false,
  pulse: false,
  localAi: false,
  tray: false,
  webAccess: false,
  plugins: false,
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
  | { kind: "permission"; id: string; title: string; action: string; reason: string }
  /** A permission card answered away from the page, such as Deny on its notification. */
  | { kind: "answered"; id: string; answer: Answer }
  | { kind: "error"; message: string }
  | { kind: "done"; stop: string }
  | { kind: "plan"; text: string }
  /** One-tap follow-ups for the reply that just ended; most replies get none. */
  | { kind: "suggestions"; items: Suggestion[] }
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

/** A follow-up to send with one tap: `prompt` becomes the next message. */
export type Suggestion = { label: string; prompt: string };

/** One answer on a question card; the label is what gets sent. */
export type ElicitChoice = { label: string; description: string; recommended: boolean };

/** The composer's chip: Chat, Plan (read-only until the plan is approved) or
 * btw (a look-only side ask). */
export type SessionMode = "chat" | "plan" | "btw";

export type ElicitAction = "accept" | "decline" | "cancel";

export interface ChatEnvelope {
  conversationId: string;
  event: ChatEvent;
}

export const CHAT_EVENT = "chat://event";

export function getSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings");
}

export type Theme = "system" | "dark" | "ops-dark" | "oled-black" | "soft-charcoal" | "light";

export function setTheme(theme: Theme): Promise<Settings> {
  return invoke<Settings>("set_theme", { theme });
}

export function saveSettings(
  model: string,
  closeToTray: boolean,
  workspace: string,
  approvals: Approvals,
  desktopControl: boolean,
): Promise<Settings> {
  return invoke<Settings>("save_settings", {
    model,
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

/** Where a model in the picker came from. */
export type ModelSource = "catalog" | "account" | "custom";

export type ModelOption = { id: string; label: string; source: ModelSource };

export type ModelList = {
  current: string;
  options: ModelOption[];
  /** Why the account's models are missing, when they are. */
  note: string | null;
};

/** The chat models to pick from; `refresh` skips the 10-minute cache. */
export function listModels(refresh = false): Promise<ModelList> {
  return invoke<ModelList>("list_models", { refresh });
}

/** Saves the chat model for the next message. */
export function setModel(id: string): Promise<Settings> {
  return invoke<Settings>("set_model", { id });
}

export function setApiKey(key: string): Promise<Settings> {
  return invoke<Settings>("set_api_key", { key });
}

export function clearApiKey(): Promise<Settings> {
  return invoke<Settings>("clear_api_key");
}

/** Runs one prompt through the agent loop; resolves with the stop reason. */
export function sendChat(
  conversationId: string,
  text: string,
  mode: SessionMode = "chat",
): Promise<string> {
  return invoke<string>("send_chat", { conversationId, text, mode });
}

/** Answers a plan, a question or a connector form; `value` goes under `field`. */
export function answerElicit(
  conversationId: string,
  id: string,
  action: ElicitAction,
  field: string | null = null,
  value = "",
): Promise<void> {
  return invoke<void>("answer_elicit", { conversationId, id, action, field, value });
}

export function answerPermission(
  conversationId: string,
  callId: string,
  answer: Answer,
): Promise<void> {
  return invoke<void>("answer_permission", { conversationId, callId, answer });
}

export function cancelChat(conversationId: string): Promise<void> {
  return invoke<void>("cancel_chat", { conversationId });
}

export function closeChat(conversationId: string): Promise<void> {
  return invoke<void>("close_chat", { conversationId });
}

export function onChatEvent(handler: (envelope: ChatEnvelope) => void): Promise<UnlistenFn> {
  return subscribe<ChatEnvelope>(CHAT_EVENT, handler);
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
}

export interface RunResult {
  text: string;
  error: string;
  stop: string;
}

/** The local wall clock, so next-run times read in the user's own zone. */
export function wallClock(now: Date = new Date()) {
  return { weekday: now.getDay(), hour: now.getHours(), minute: now.getMinutes() };
}

export function listAutomations(): Promise<AutomationRow[]> {
  return invoke<AutomationRow[]>("list_automations", { clock: wallClock() });
}

export function addAutomation(text: string): Promise<AutomationRow[]> {
  return invoke<AutomationRow[]>("add_automation", { text, clock: wallClock() });
}

export function setAutomationEnabled(id: string, enabled: boolean): Promise<AutomationRow[]> {
  return invoke<AutomationRow[]>("set_automation_enabled", { id, enabled, clock: wallClock() });
}

export function removeAutomation(id: string): Promise<AutomationRow[]> {
  return invoke<AutomationRow[]>("remove_automation", { id, clock: wallClock() });
}

export function runAutomation(id: string): Promise<RunResult> {
  return invoke<RunResult>("run_automation", { id });
}

export interface MemoryRow {
  id: string;
  text: string;
  /** Why this line is here, including the chat turn when it has one. */
  source: string;
  /** When the line was written or last changed. */
  updated: string;
  proposed: boolean;
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
  | { status: "failed"; error: string };

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
  | { kind: "open"; view: "settings" | "memory" | "automations" | "connectors" }
  | { kind: "modelSet"; id: string; text: string }
  | { kind: "notice"; text: string };

export function slashHits(draft: string): Promise<SlashHit[]> {
  return invoke<SlashHit[]>("slash_hits", { draft });
}

export function runSlash(line: string): Promise<SlashEffect> {
  return invoke<SlashEffect>("run_slash", { line });
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

/** One Feed post: something that happened or was found. */
export interface PulsePost {
  id: string;
  source: string;
  title: string;
  summary: string;
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
}

export interface PulsePage {
  feed: PulsePost[];
  /** An empty label is the unlabeled top group. */
  ideas: { label: string; ideas: PulseIdea[] }[];
}

export type PulseAct =
  | "dismiss"
  | "notThis"
  | "love"
  | "delete"
  | "snooze"
  | "accept"
  | "moreLike"
  | "notInterested";

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

/** Discuss: the text a chat starts from, carrying the post. */
export function pulseDiscuss(id: string): Promise<string> {
  return invoke<string>("pulse_discuss", { id });
}

export function pulseWhy(id: string): Promise<string> {
  return invoke<string>("pulse_why", { id });
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

export function journalToMission(id: string, conversationId: string): Promise<JournalMission> {
  return invoke<JournalMission>("journal_to_mission", { id, conversationId });
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
  /** "auto", "on" or "off". */
  draft: string;
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

export interface LocalStatus {
  tier: string;
  model: string;
  state: string;
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

export interface LocalEngineInfo {
  /** ready | missing | downloading | noBuild */
  state: string;
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
  picker: { models: LocalModelRow[]; onThisPc: LocalFileRow[]; selected: string };
  /** What hardware detection couldn't see. */
  limits: string[];
  maxThreads: number;
  engine: LocalEngineInfo;
  draftOffer: LocalDraftOffer | null;
  /** Offers for every catalog model still missing its draft pair. */
  draftOffers: LocalDraftOffer[];
  draftProgress: LocalDraftProgress | null;
}

export type LocalEngineEvent = {
  state: string;
  progress: LocalEngineProgress | null;
  error: string | null;
  message: string | null;
};

export type LocalDrafterEvent = {
  state: string;
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

export function runLocalSpeedTest(): Promise<string> {
  return invoke<string>("run_local_speed_test");
}

export function startLocalEngineDownload(): Promise<LocalEngineInfo> {
  return invoke<LocalEngineInfo>("start_local_engine_download");
}

export function cancelLocalEngineDownload(): Promise<void> {
  return invoke("cancel_local_engine_download");
}

export function startLocalDrafterDownload(targetId: string): Promise<LocalDraftOffer | null> {
  return invoke<LocalDraftOffer | null>("start_local_drafter_download", { targetId });
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
  /** Adults-only personalities: off leaves the explicit presets out. */
  explicit: boolean;
}

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

export function setExplicitPersonalities(on: boolean): Promise<PersonalityPage> {
  return invoke<PersonalityPage>("set_explicit_personalities", { on });
}

/** The copy's id, then the page. */
export function duplicatePersonality(id: string): Promise<[string, PersonalityPage]> {
  return invoke<[string, PersonalityPage]>("duplicate_personality", { id });
}

export function savePersonality(edit: PersonalityEdit): Promise<PersonalityPage> {
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

export type InterestAct = { kind: "pin"; on: boolean } | { kind: "mute"; on: boolean } | { kind: "remove" };

export function getInterests(): Promise<InterestPage> {
  return invoke<InterestPage>("get_interests");
}

export function addInterest(text: string): Promise<InterestPage> {
  return invoke<InterestPage>("add_interest", { text });
}

export function interestAct(topic: string, act: InterestAct): Promise<InterestPage> {
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

export function addPermissionRule(rule: string, kind: RuleKind): Promise<PermissionPage> {
  return invoke<PermissionPage>("add_permission_rule", { rule, kind });
}

export function removePermissionRule(index: number, rule: string): Promise<PermissionPage> {
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

export function onUpdateAvailable(handler: (view: UpdateView) => void): Promise<UnlistenFn> {
  return subscribe<UpdateView>(UPDATE_EVENT, handler);
}

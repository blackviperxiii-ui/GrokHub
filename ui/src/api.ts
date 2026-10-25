// Typed wrappers over the Tauri commands in src-tauri/src/lib.rs.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** How chat handles tools that change things (the cabin's permission mode). */
export type Approvals = "ask" | "auto" | "always";

export interface Settings {
  model: string;
  closeToTray: boolean;
  hasApiKey: boolean;
  /** The folder picked in Settings, empty for the default. */
  workspace: string;
  /** The folder the agent actually works in. */
  workspacePath: string;
  approvals: Approvals;
}

export type Answer = "allow" | "always" | "deny";

/** One engine event for a conversation, from `chat://event`. */
export type ChatEvent =
  | { kind: "text"; text: string }
  | { kind: "tool"; id: string; title: string; status: string; detail: string }
  | { kind: "permission"; id: string; title: string; action: string; reason: string }
  | { kind: "error"; message: string }
  | { kind: "done"; stop: string };

export interface ChatEnvelope {
  conversationId: string;
  event: ChatEvent;
}

export const CHAT_EVENT = "chat://event";

export function getSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings");
}

export function saveSettings(
  model: string,
  closeToTray: boolean,
  workspace: string,
  approvals: Approvals,
): Promise<Settings> {
  return invoke<Settings>("save_settings", { model, closeToTray, workspace, approvals });
}

export function setApiKey(key: string): Promise<Settings> {
  return invoke<Settings>("set_api_key", { key });
}

export function clearApiKey(): Promise<Settings> {
  return invoke<Settings>("clear_api_key");
}

/** Runs one prompt through the agent loop; resolves with the stop reason. */
export function sendChat(conversationId: string, text: string): Promise<string> {
  return invoke<string>("send_chat", { conversationId, text });
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
  return listen<ChatEnvelope>(CHAT_EVENT, (event) => handler(event.payload));
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

export interface MemoryNotes {
  global: string;
  workspace: string;
  globalPath: string;
  workspacePath: string;
  /** The cabin's AMR store is on, so recall does not read these files. */
  amr: boolean;
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

export function addMemory(
  text: string,
  global: boolean,
): Promise<{ status: string; notes: MemoryNotes }> {
  return invoke("add_memory", { text, global });
}

export function searchMemory(query: string): Promise<MemoryHit[]> {
  return invoke<MemoryHit[]>("search_memory", { query });
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
  /** "2h ago". */
  ago: string;
  /** The first source link, empty when there is none. */
  link: string;
  liked: boolean;
}

/** One Ideas row: something GrokHub could do next. */
export interface PulseIdea {
  id: string;
  /** The bold "I can …" line. */
  title: string;
  summary: string;
  /** Do, Automate, Learn or Watch. */
  kind: string;
  /** What Run puts in the chat box. */
  action: string;
}

export interface PulsePage {
  feed: PulsePost[];
  /** An empty label is the unlabeled top group. */
  ideas: { label: string; ideas: PulseIdea[] }[];
}

export type PulseAct = "dismiss" | "notThis" | "like" | "snooze" | "accept";

/** Today's local date for the ledger line, and the time Snooze counts from. */
export function localDay(now: Date = new Date()) {
  const pad = (n: number) => String(n).padStart(2, "0");
  return {
    date: `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`,
    hour: now.getHours(),
    minute: now.getMinutes(),
  };
}

export function getPulse(): Promise<PulsePage> {
  return invoke<PulsePage>("get_pulse");
}

export function pulseAct(id: string, act: PulseAct): Promise<PulsePage> {
  return invoke<PulsePage>("pulse_act", { id, act, day: localDay() });
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

export interface LocalAiView {
  config: LocalAiConfig;
  status: LocalStatus;
  picker: { models: LocalModelRow[]; onThisPc: LocalFileRow[]; selected: string };
  /** What hardware detection couldn't see. */
  limits: string[];
  maxThreads: number;
}

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

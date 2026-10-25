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

// Typed wrappers over the Tauri commands in src-tauri/src/lib.rs.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface Settings {
  model: string;
  closeToTray: boolean;
  hasApiKey: boolean;
  /** The folder picked in Settings, empty for the default. */
  workspace: string;
  /** The folder the agent actually works in. */
  workspacePath: string;
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
): Promise<Settings> {
  return invoke<Settings>("save_settings", { model, closeToTray, workspace });
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

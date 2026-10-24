// Typed wrappers over the Tauri commands in src-tauri/src/lib.rs.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Role = "user" | "assistant";

export interface ChatMessage {
  role: Role;
  text: string;
}

export interface Settings {
  model: string;
  closeToTray: boolean;
  hasApiKey: boolean;
}

export interface ChatDelta {
  conversationId: string;
  text: string;
}

export const DELTA_EVENT = "chat://delta";

export function getSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings");
}

export function saveSettings(model: string, closeToTray: boolean): Promise<Settings> {
  return invoke<Settings>("save_settings", { model, closeToTray });
}

export function setApiKey(key: string): Promise<Settings> {
  return invoke<Settings>("set_api_key", { key });
}

export function clearApiKey(): Promise<Settings> {
  return invoke<Settings>("clear_api_key");
}

/** Sends the whole conversation; resolves with the full reply once the stream ends. */
export function sendChat(conversationId: string, messages: ChatMessage[]): Promise<string> {
  return invoke<string>("send_chat", { conversationId, messages });
}

export function cancelChat(conversationId: string): Promise<void> {
  return invoke<void>("cancel_chat", { conversationId });
}

export function onChatDelta(handler: (delta: ChatDelta) => void): Promise<UnlistenFn> {
  return listen<ChatDelta>(DELTA_EVENT, (event) => handler(event.payload));
}

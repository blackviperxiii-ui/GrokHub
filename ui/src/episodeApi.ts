// Desktop episodes (src-tauri/src/episode.rs): the supervised observe, act,
// verify session the cabin shows in its Work tree. Desktop only: the web app
// and the tests get nothing and show nothing.
import type { UnlistenFn } from "@tauri-apps/api/event";
import { call, inShell, subscribe } from "./transport";

export const EPISODE_EVENT = "episode://event";

export interface EpisodeStep {
  n: number;
  tool: string;
  decision: string;
  result: string;
  goalStep: string;
}

/** A hard-class step (delete, send, money, credentials) waiting for an OK. */
export interface EpisodePark {
  id: string;
  tool: string;
  action: string;
  class: string;
}

export interface EpisodePanel {
  conversationId: string;
  episodeId: string;
  /** "Desktop session · 12 steps · 4 min". */
  header: string;
  steps: EpisodeStep[];
  parks: EpisodePark[];
  /** The latest screenshot as a data URL; null when there is none, or when
   * the screen may show a secret. */
  frame: string | null;
  state: "running" | "paused" | "ended";
  ended: string | null;
  /** Steps run through the Cua driver's gate. */
  cua: boolean;
}

export function onEpisode(handler: (panel: EpisodePanel) => void): Promise<UnlistenFn> {
  if (!inShell()) return Promise.resolve(() => {});
  return subscribe<EpisodePanel>(EPISODE_EVENT, handler);
}

export function episodeState(conversationId: string): Promise<EpisodePanel | null> {
  if (!inShell()) return Promise.resolve(null);
  return call<EpisodePanel | null>("episode_state", { conversationId });
}

export function episodeAnswer(conversationId: string, parkId: string, approve: boolean): Promise<void> {
  return call<void>("episode_answer", { conversationId, parkId, approve });
}

/** Pause after the step that is running; the next message runs it again. */
export function episodePause(conversationId: string): Promise<boolean> {
  return call<boolean>("episode_pause", { conversationId });
}

/** Stop: nothing more runs and every waiting card is denied. */
export function episodeStop(conversationId: string): Promise<boolean> {
  return call<boolean>("episode_stop", { conversationId });
}

/** Arms or disarms the chat's supervised desktop session. Resolves with
 * whether it is armed now. */
export function episodeArm(conversationId: string, on: boolean): Promise<boolean> {
  return call<boolean>("episode_arm", { conversationId, on });
}

export function episodeArmed(conversationId: string): Promise<boolean> {
  if (!inShell()) return Promise.resolve(false);
  return call<boolean>("episode_armed", { conversationId });
}

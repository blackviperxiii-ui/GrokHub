// A chat that paused because no model in your plan answered picks its ask
// back up on its own once one does (the router's no-route wait). The app
// re-checks in the background and says when one is back; the open chat then
// takes its resume, at most once.
import type { UnlistenFn } from "@tauri-apps/api/event";
import { call, subscribe } from "./transport";

/** Sent when a model in your plan answers again after a wait. */
export const MODEL_BACK_EVENT = "router://model-back";

/** The paused ask to send again, and the quiet note that says why. */
export interface ModelResume {
  conversationId: string;
  ask: string;
  note: string;
}

/** This chat's resume, once a model is back; null otherwise or once taken. */
export function modelResume(conversationId: string): Promise<ModelResume | null> {
  return call<ModelResume | null>("model_resume", { conversationId });
}

export function onModelBack(handler: () => void): Promise<UnlistenFn> {
  return subscribe<ModelResume>(MODEL_BACK_EVENT, () => handler());
}

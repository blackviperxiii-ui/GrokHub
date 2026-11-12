import type { StageOutcome } from "./api";
import { call as invoke } from "./transport";

/** Explain was tapped after the file was moved or deleted. */
export const SHOT_GONE = "That screenshot is gone.";
/** Explain was tapped after the offer ran out or the switch went off. */
export const OFFER_ENDED = "That screenshot offer has ended.";

/** The composer's screenshot offer (src-tauri/src/screenshot_offer.rs). */
export interface ScreenshotOfferChip {
  /** The file's name, for the chip's title. */
  name: string;
  /** "Explain this?" */
  label: string;
  /** "Not now" */
  dismiss: string;
  /** What goes in an empty composer once the picture is attached. */
  prompt: string;
}

export interface ScreenshotOfferView {
  /** Settings → Behavior → Screenshot explain. Off by default. */
  enabled: boolean;
  offer: ScreenshotOfferChip | null;
}

/** One look at the screenshot folders (only names and times are read). */
export function getScreenshotOffer(): Promise<ScreenshotOfferView> {
  return invoke<ScreenshotOfferView>("get_screenshot_offer");
}

export function setScreenshotExplain(on: boolean): Promise<ScreenshotOfferView> {
  return invoke<ScreenshotOfferView>("set_screenshot_explain", { on });
}

export function dismissScreenshotOffer(): Promise<ScreenshotOfferView> {
  return invoke<ScreenshotOfferView>("dismiss_screenshot_offer");
}

/** Attaches the offered screenshot to the composer. Nothing is sent. */
export function explainScreenshotOffer(conversationId: string): Promise<StageOutcome> {
  return invoke<StageOutcome>("explain_screenshot_offer", { conversationId });
}

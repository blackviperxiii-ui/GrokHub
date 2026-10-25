// The Settings sections, apart from the page so the command palette can list
// them without loading Settings.
import type { Capabilities } from "./api";

export type Section = "general" | "account" | "personality" | "playbooks" | "localAi" | "webAccess" | "labs" | "about";

export const SECTION_LABELS: Record<Section, string> = {
  general: "General",
  account: "Account",
  personality: "Personality",
  playbooks: "Playbooks",
  localAi: "Local AI",
  webAccess: "Web access",
  labs: "Labs",
  about: "About",
};

/** The sections this platform has, in nav order. */
export function sections(caps: Capabilities): Section[] {
  return [
    "general",
    "account",
    "personality",
    "playbooks",
    ...(caps.localAi ? (["localAi"] as const) : []),
    ...(caps.webAccess ? (["webAccess"] as const) : []),
    ...(caps.plugins ? (["labs"] as const) : []),
    "about",
  ];
}

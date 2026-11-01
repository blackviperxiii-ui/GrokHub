// The Settings sections, apart from the page so the command palette can list
// them without loading Settings.
import type { Capabilities } from "./api";

export type Section = "general" | "account" | "defaults" | "behavior" | "appearance" | "personality" | "playbooks" | "permissions" | "privacy" | "background" | "pulse" | "localAi" | "webAccess" | "labs" | "about";

export const SECTION_LABELS: Record<Section, string> = {
  general: "General",
  account: "Account",
  defaults: "Defaults",
  behavior: "Behavior",
  appearance: "Appearance",
  personality: "Personality",
  playbooks: "Playbooks",
  permissions: "Permissions",
  privacy: "Privacy & data",
  background: "Background",
  pulse: "Pulse",
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
    "defaults",
    "behavior",
    "appearance",
    "personality",
    "playbooks",
    "permissions",
    "privacy",
    "background",
    ...(caps.pulse ? (["pulse"] as const) : []),
    ...(caps.localAi ? (["localAi"] as const) : []),
    ...(caps.webAccess ? (["webAccess"] as const) : []),
    ...(caps.plugins ? (["labs"] as const) : []),
    "about",
  ];
}

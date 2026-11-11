// The Settings sections, apart from the page so the command palette can list
// them without loading Settings.
import type { Capabilities } from "./api";

export type Section = "general" | "account" | "defaults" | "behavior" | "appearance" | "personality" | "playbooks" | "permissions" | "privacy" | "background" | "autoSetup" | "code" | "pulse" | "localAi" | "aiModels" | "webAccess" | "labs" | "about";

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
  autoSetup: "Auto setup",
  code: "Code",
  pulse: "Pulse",
  localAi: "Local AI",
  aiModels: "AI models",
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
    ...(caps.autoSetup ? (["autoSetup"] as const) : []),
    ...(caps.code ? (["code"] as const) : []),
    ...(caps.pulse ? (["pulse"] as const) : []),
    ...(caps.localAi ? (["localAi"] as const) : []),
    ...(caps.localAi ? (["aiModels"] as const) : []),
    ...(caps.webAccess ? (["webAccess"] as const) : []),
    ...(caps.plugins ? (["labs"] as const) : []),
    "about",
  ];
}

/** Settings' top row: a few plain-word categories, each holding a few pages. */
export type SettingsCategory = "account" | "look" | "ai" | "pc" | "privacy" | "help";

export const CATEGORY_LABELS: Record<SettingsCategory, string> = {
  account: "Account",
  look: "Look & feel",
  ai: "AI & chat",
  pc: "Your PC",
  privacy: "Privacy & safety",
  help: "Help & updates",
};

/** Every section sits in exactly one category, in side-menu order. Matches the
 * cabin's Settings so both apps read the same. */
const CATEGORY_SECTIONS: Record<SettingsCategory, readonly Section[]> = {
  account: ["account"],
  look: ["appearance"],
  ai: ["defaults", "personality", "playbooks", "pulse", "localAi", "aiModels"],
  pc: ["general", "behavior", "background", "autoSetup", "code"],
  privacy: ["privacy", "permissions", "webAccess"],
  // About first: the version is what people come here for.
  help: ["about", "labs"],
};

const CATEGORY_ORDER: readonly SettingsCategory[] = ["account", "look", "ai", "pc", "privacy", "help"];

/** The category a section sits in, so the top row lights the right button. */
export function categoryOf(section: Section): SettingsCategory {
  return CATEGORY_ORDER.find((cat) => CATEGORY_SECTIONS[cat].includes(section)) ?? "pc";
}

/** This platform's pages in one category, in side-menu order. */
export function categorySections(caps: Capabilities, category: SettingsCategory): Section[] {
  const here = new Set(sections(caps));
  return CATEGORY_SECTIONS[category].filter((section) => here.has(section));
}

/** The categories with at least one page on this platform, in top-row order. */
export function categories(caps: Capabilities): SettingsCategory[] {
  return CATEGORY_ORDER.filter((cat) => categorySections(caps, cat).length > 0);
}

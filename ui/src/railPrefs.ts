/** Sidebar v2 prefs: Simple vs Advanced, pins, and which sections are folded.
 * Kept in localStorage so the rail remembers without a shell.json bump. */
import type { View } from "./Palette";

export type RailMode = "simple" | "advanced";

export type RailSectionId = "core" | "work" | "make" | "you" | "pc";

export interface RailSectionDef {
  id: RailSectionId;
  label: string;
  views: readonly View[];
}

/** Advanced-mode groups. Settings stays at the rail foot, not in a group. */
export const RAIL_SECTIONS: readonly RailSectionDef[] = [
  { id: "core", label: "Core", views: ["home", "chat", "code", "library"] },
  {
    id: "work",
    label: "Work",
    views: ["workboard", "automations", "agents", "team", "objectives", "command"],
  },
  { id: "make", label: "Make", views: ["skills", "connectors", "mods"] },
  { id: "you", label: "You", views: ["memory", "pulse", "journal", "meetings", "history"] },
  { id: "pc", label: "PC", views: ["devices", "fleet", "updates"] },
];

export interface RailPrefs {
  mode: RailMode;
  /** Pages pinned to the top of the rail, in order. Settings cannot be pinned. */
  pinned: View[];
  /** Section ids the person folded shut. */
  collapsed: RailSectionId[];
}

export const DEFAULT_RAIL_PREFS: RailPrefs = {
  mode: "simple",
  pinned: [],
  collapsed: [],
};

const STORAGE_KEY = "shell.railPrefs";

/** Fired on `window` after prefs are saved so open Settings and the rail stay in sync. */
export const RAIL_PREFS_EVENT = "shell-rail-prefs";

const SECTION_IDS: readonly RailSectionId[] = RAIL_SECTIONS.map((s) => s.id);

function isView(value: unknown): value is View {
  return typeof value === "string" && value.length > 0 && value !== "settings";
}

function isSectionId(value: unknown): value is RailSectionId {
  return typeof value === "string" && (SECTION_IDS as readonly string[]).includes(value);
}

/** Read prefs from localStorage; a bad or missing value becomes the default. */
export function loadRailPrefs(): RailPrefs {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return { ...DEFAULT_RAIL_PREFS, pinned: [], collapsed: [] };
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") {
      return { ...DEFAULT_RAIL_PREFS, pinned: [], collapsed: [] };
    }
    const obj = parsed as Record<string, unknown>;
    const mode: RailMode = obj.mode === "advanced" ? "advanced" : "simple";
    const pinned = Array.isArray(obj.pinned) ? obj.pinned.filter(isView) : [];
    const collapsed = Array.isArray(obj.collapsed) ? obj.collapsed.filter(isSectionId) : [];
    return { mode, pinned, collapsed };
  } catch {
    return { ...DEFAULT_RAIL_PREFS, pinned: [], collapsed: [] };
  }
}

/** Persist prefs. Storage failures are ignored (private windows). */
export function saveRailPrefs(prefs: RailPrefs): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(prefs));
  } catch {
    // Storage can be off; the in-memory prefs still apply.
  }
  if (typeof window !== "undefined") {
    window.dispatchEvent(new CustomEvent(RAIL_PREFS_EVENT, { detail: prefs }));
  }
}

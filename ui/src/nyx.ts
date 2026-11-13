// Nyx in the shell: which picture the chat's face and dock show. The desktop
// picks the set for the voice in use and its adults-only gate (src-tauri's
// nyx_look); this file turns the set's repo paths into bundled image URLs
// and keeps the pose still until what the chat does changes.
import { useEffect, useRef, useState } from "react";
import type { Item } from "./Chat";
import { call } from "./transport";

/** What the chat can be doing. The shell watches no games, so no Gaming. */
export type Mood = "working" | "doctor" | "reading" | "writing" | "thinking" | "hanging-out";

/** One set of poses as repo paths (`brand/nyx/actions/coding/2.jpg`). */
export interface NyxLook {
  set: string;
  poses: string[];
  explicit: boolean;
}

/** The picture on show, and the mood it was picked for. */
export interface NyxShown {
  url: string;
  set: string;
  mood: Mood;
}

export const MOOD_LABEL: Record<Mood, string> = {
  working: "Working",
  doctor: "Looking after your PC",
  reading: "Reading",
  writing: "Writing",
  thinking: "Thinking",
  "hanging-out": "Hanging out",
};

// Every picture of hers, by URL. Only the URLs are in the script; a picture
// loads when an <img> shows it. The gaming set never shows in the shell, so
// it is left out of the bundle.
const ART = import.meta.glob<string>(
  [
    "../../brand/nyx/**/*.jpg",
    "../../brand/nyx-*.jpg",
    "!../../brand/nyx/actions/gaming/**",
    "!../../brand/nyx-gaming.jpg",
  ],
  { query: "?url", import: "default", eager: true },
);

/** The bundled URL for a repo path, or null for anything not bundled. */
export function artUrl(path: string): string | null {
  return Object.hasOwn(ART, `../../${path}`) ? ART[`../../${path}`] : null;
}

/** What the chat is doing, as one string that changes only when the activity
 * does: `tool:<title>` for a running tool, else a mood. Text streaming in
 * leaves it alone. */
export function activityOf(items: Item[], busy: boolean): string {
  if (!busy) return "hanging-out";
  const last = items[items.length - 1];
  if (last?.kind === "permission" && last.answer === null) return "working";
  if (last?.kind === "tool") return `tool:${last.title}`;
  return "thinking";
}

/** The mood a tool title or mood activity stands for, as nyx_look reads it. */
function lookArgs(activity: string): { tool?: string; mood?: Mood } {
  return activity.startsWith("tool:") ? { tool: activity.slice(5) } : { mood: activity as Mood };
}

/** The desktop's set for the voice in use; null when that voice has no art. */
export function nyxLook(activity: string): Promise<(NyxLook & { mood: Mood }) | null> {
  return call<(NyxLook & { mood: Mood }) | null>("nyx_look", lookArgs(activity));
}

/** Settings → Personality's picture paths, by preset id. Desktop only. */
export function nyxTiles(ids: string[]): Promise<Record<string, string>> {
  return call<Record<string, string>>("nyx_tiles", { ids });
}

/** Which pose of a set shows. The first time a set comes on it shows its
 * first pose; each later time the set or the mood changes, that set moves on
 * one pose, wrapping at the end. While both hold, the pose holds. */
export class PoseCycle {
  private shown: { set: string; mood: string; i: number } | null = null;
  private readonly turns = new Map<string, number>();

  pick(set: string, mood: string, len: number): number {
    const n = Math.max(1, len);
    if (this.shown && this.shown.set === set && this.shown.mood === mood) return this.shown.i % n;
    const turn = this.turns.get(set) ?? 0;
    this.turns.set(set, turn + 1);
    const i = turn % n;
    this.shown = { set, mood, i };
    return i;
  }
}

/** The picture for `activity`, asked again when `refresh` changes (the Chat
 * tab coming back, where the voice or the gate may have changed). Null while
 * the voice has no art or the desktop can't be reached. */
export function useNyx(activity: string, refresh: unknown = null): NyxShown | null {
  const cycle = useRef(new PoseCycle());
  const [shown, setShown] = useState<NyxShown | null>(null);
  useEffect(() => {
    let live = true;
    void (async () => {
      try {
        const look = await nyxLook(activity);
        if (!live) return;
        if (!look || look.poses.length === 0) {
          setShown(null);
          return;
        }
        const i = cycle.current.pick(look.set, look.mood, look.poses.length);
        const url = artUrl(look.poses[i]);
        if (live) setShown(url ? { url, set: look.set, mood: look.mood } : null);
      } catch {
        if (live) setShown(null);
      }
    })();
    return () => {
      live = false;
    };
  }, [activity, refresh]);
  return shown;
}

/** Settings → Personality's pictures as URLs, by preset id. Asked again when
 * the presets or the adults-only gate change. */
export function useNyxTiles(ids: string[] | null, gate: boolean): Record<string, string> {
  const [urls, setUrls] = useState<Record<string, string>>({});
  const key = ids?.join("\n") ?? null;
  useEffect(() => {
    if (key === null) return;
    let live = true;
    void (async () => {
      try {
        const paths = await nyxTiles(key ? key.split("\n") : []);
        const next: Record<string, string> = {};
        for (const [id, path] of Object.entries(paths)) {
          const url = artUrl(path);
          if (url) next[id] = url;
        }
        if (live) setUrls(next);
      } catch {
        if (live) setUrls({});
      }
    })();
    return () => {
      live = false;
    };
  }, [key, gate]);
  return urls;
}

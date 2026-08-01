/**
 * Adaptive memory for quick-assist chips.
 * Learns from chip clicks + free-typed prompts; boosts ranking over time.
 */
import type { QuickChip, QuickChipKind } from "./quick-assistant";

export type QuickAssistHit = {
  /** Stable key: preferred chip id, else hashed value */
  key: string;
  label: string;
  value: string;
  kind: QuickChipKind;
  /** Times selected via chip or matched from free text */
  uses: number;
  /** Times originated from free-typed prompts (not chip) */
  typedUses: number;
  lastUsedAt: number;
  /** Rolling hour-of-day affinity (0–23) for mild time-of-day boost */
  hourHits: number[];
};

export type QuickAssistMemory = {
  version: 1;
  hits: QuickAssistHit[];
  /** Co-occurrence: after using key A, boost keys that followed */
  transitions: Record<string, Record<string, number>>;
  lastChipKey: string | null;
  totalEvents: number;
  updatedAt: number;
};

const MAX_HITS = 80;
const MAX_TRANSITIONS_PER_KEY = 12;

export function emptyQuickAssistMemory(): QuickAssistMemory {
  return {
    version: 1,
    hits: [],
    transitions: {},
    lastChipKey: null,
    totalEvents: 0,
    updatedAt: Date.now(),
  };
}

export function normalizeMemory(raw: unknown): QuickAssistMemory {
  const empty = emptyQuickAssistMemory();
  if (!raw || typeof raw !== "object") return empty;
  const m = raw as Partial<QuickAssistMemory>;
  if (m.version !== 1 || !Array.isArray(m.hits)) return empty;
  return {
    version: 1,
    hits: m.hits
      .filter((h) => h && typeof h.key === "string" && h.value)
      .slice(0, MAX_HITS)
      .map((h) => ({
        key: h.key,
        label: String(h.label || h.value).slice(0, 48),
        value: String(h.value).slice(0, 400),
        kind: (["chat", "shell", "nav", "mode"].includes(h.kind)
          ? h.kind
          : "chat") as QuickChipKind,
        uses: Math.max(0, Number(h.uses) || 0),
        typedUses: Math.max(0, Number(h.typedUses) || 0),
        lastUsedAt: Number(h.lastUsedAt) || 0,
        hourHits: Array.isArray(h.hourHits)
          ? h.hourHits.slice(0, 24).map((n) => Math.max(0, Number(n) || 0))
          : [],
      })),
    transitions:
      m.transitions && typeof m.transitions === "object" ? m.transitions : {},
    lastChipKey: typeof m.lastChipKey === "string" ? m.lastChipKey : null,
    totalEvents: Math.max(0, Number(m.totalEvents) || 0),
    updatedAt: Number(m.updatedAt) || Date.now(),
  };
}

function valueKey(value: string, kind?: QuickChipKind): string {
  const v = value.trim().toLowerCase().replace(/\s+/g, " ");
  // Collapse similar shell/chat phrasing
  return `${kind || "x"}:${v.slice(0, 160)}`;
}

export function chipMemoryKey(chip: Pick<QuickChip, "id" | "value" | "kind">): string {
  // Prefer stable id for built-ins; value key for freeform
  if (chip.id.startsWith("learn-") || chip.id.startsWith("recent-")) {
    return valueKey(chip.value, chip.kind);
  }
  return `id:${chip.id}`;
}

function bumpHour(hourHits: number[], hour: number): number[] {
  const arr = hourHits.length === 24 ? [...hourHits] : Array.from({ length: 24 }, () => 0);
  arr[hour] = (arr[hour] || 0) + 1;
  return arr;
}

function upsertHit(
  memory: QuickAssistMemory,
  hit: Omit<QuickAssistHit, "uses" | "typedUses" | "lastUsedAt" | "hourHits"> & {
    usesDelta?: number;
    typedDelta?: number;
  },
): QuickAssistMemory {
  const hour = new Date().getHours();
  const existing = memory.hits.find((h) => h.key === hit.key);
  let hits: QuickAssistHit[];
  if (existing) {
    hits = memory.hits.map((h) =>
      h.key === hit.key
        ? {
            ...h,
            label: hit.label || h.label,
            value: hit.value || h.value,
            kind: hit.kind || h.kind,
            uses: h.uses + (hit.usesDelta ?? 1),
            typedUses: h.typedUses + (hit.typedDelta ?? 0),
            lastUsedAt: Date.now(),
            hourHits: bumpHour(h.hourHits, hour),
          }
        : h,
    );
  } else {
    hits = [
      {
        key: hit.key,
        label: hit.label,
        value: hit.value,
        kind: hit.kind,
        uses: hit.usesDelta ?? 1,
        typedUses: hit.typedDelta ?? 0,
        lastUsedAt: Date.now(),
        hourHits: bumpHour([], hour),
      },
      ...memory.hits,
    ];
  }
  // Prune least valuable
  hits.sort((a, b) => {
    const sa = a.uses * 2 + a.typedUses + a.lastUsedAt / 1e13;
    const sb = b.uses * 2 + b.typedUses + b.lastUsedAt / 1e13;
    return sb - sa;
  });
  hits = hits.slice(0, MAX_HITS);
  return {
    ...memory,
    hits,
    totalEvents: memory.totalEvents + 1,
    updatedAt: Date.now(),
  };
}

function recordTransition(
  memory: QuickAssistMemory,
  fromKey: string | null,
  toKey: string,
): QuickAssistMemory {
  if (!fromKey || fromKey === toKey) {
    return { ...memory, lastChipKey: toKey };
  }
  const transitions = { ...memory.transitions };
  const row = { ...(transitions[fromKey] || {}) };
  row[toKey] = (row[toKey] || 0) + 1;
  // prune row
  const entries = Object.entries(row).sort((a, b) => b[1] - a[1]).slice(0, MAX_TRANSITIONS_PER_KEY);
  transitions[fromKey] = Object.fromEntries(entries);
  // cap total transition keys
  const keys = Object.keys(transitions);
  if (keys.length > MAX_HITS) {
    for (const k of keys.slice(MAX_HITS)) delete transitions[k];
  }
  return { ...memory, transitions, lastChipKey: toKey };
}

/** User tapped a quick chip. */
export function rememberChipClick(
  memory: QuickAssistMemory,
  chip: QuickChip,
): QuickAssistMemory {
  const key = chipMemoryKey(chip);
  let next = upsertHit(memory, {
    key,
    label: chip.label,
    value: chip.value,
    kind: chip.kind,
    usesDelta: 1,
  });
  next = recordTransition(next, memory.lastChipKey, key);
  return next;
}

/**
 * Free-typed prompt — reinforce similar learned chips and create habits.
 * Shell commands and short repeatable prompts become learnable chips.
 */
export function rememberTypedPrompt(
  memory: QuickAssistMemory,
  text: string,
): QuickAssistMemory {
  const raw = text.trim();
  if (!raw || raw.length < 2) return memory;
  if (raw.startsWith("[Automation:")) return memory;

  const kind: QuickChipKind = raw.startsWith("$") || raw.startsWith("/sh ")
    ? "shell"
    : "chat";
  // Only learn compact, re-runnable prompts
  if (kind === "chat" && raw.length > 120) {
    // Still boost matching existing chips by token overlap
    return boostMatching(memory, raw);
  }

  const key = valueKey(raw, kind);
  const label =
    kind === "shell"
      ? raw.length > 28
        ? raw.slice(0, 27) + "…"
        : raw
      : raw.length > 32
        ? raw.slice(0, 31) + "…"
        : raw;

  let next = upsertHit(memory, {
    key,
    label,
    value: raw,
    kind,
    usesDelta: 1,
    typedDelta: 1,
  });
  next = recordTransition(next, memory.lastChipKey, key);
  next = boostMatching(next, raw);
  return next;
}

function boostMatching(memory: QuickAssistMemory, text: string): QuickAssistMemory {
  const lower = text.toLowerCase();
  const tokens = lower.split(/\s+/).filter((t) => t.length > 3).slice(0, 8);
  if (!tokens.length) return memory;
  const hits = memory.hits.map((h) => {
    const hay = `${h.label} ${h.value}`.toLowerCase();
    let match = 0;
    for (const tok of tokens) {
      if (hay.includes(tok)) match += 1;
    }
    if (match === 0) return h;
    return {
      ...h,
      uses: h.uses + (match >= 2 ? 0.35 : 0.15),
      lastUsedAt: h.lastUsedAt, // don't bump recency for soft match
    };
  });
  return { ...memory, hits, updatedAt: Date.now() };
}

/** Score boost for an existing chip from memory. */
export function memoryBoostForChip(
  memory: QuickAssistMemory,
  chip: QuickChip,
  now = Date.now(),
): number {
  if (!memory.hits.length) return 0;
  const key = chipMemoryKey(chip);
  const byId = memory.hits.find((h) => h.key === key);
  const byValue = memory.hits.find(
    (h) => h.value.trim().toLowerCase() === chip.value.trim().toLowerCase(),
  );
  const hit = byId || byValue;
  let boost = 0;

  if (hit) {
    // Frequency (log scale)
    boost += Math.min(48, Math.log2(1 + hit.uses) * 12);
    boost += Math.min(12, hit.typedUses * 1.5);
    // Recency: half-life ~5 days
    const ageMs = Math.max(0, now - hit.lastUsedAt);
    const days = ageMs / (86400_000);
    const recency = Math.max(0, 22 - days * 3);
    boost += recency;
    // Time-of-day affinity
    const hour = new Date(now).getHours();
    if (hit.hourHits[hour] && hit.hourHits[hour]! > 0) {
      boost += Math.min(10, hit.hourHits[hour]! * 2);
    }
  }

  // Transition: after last chip, what usually follows
  if (memory.lastChipKey && memory.transitions[memory.lastChipKey]) {
    const tKey = hit?.key || key;
    const n = memory.transitions[memory.lastChipKey]![tKey] || 0;
    if (n > 0) boost += Math.min(18, n * 4);
  }

  // Soft token match against any strong habit even if ids differ
  if (!hit && memory.hits.length) {
    const hay = `${chip.label} ${chip.value}`.toLowerCase();
    for (const h of memory.hits.slice(0, 15)) {
      if (h.uses < 2) continue;
      const other = h.value.toLowerCase();
      if (hay.includes(other.slice(0, 20)) || other.includes(hay.slice(0, 20))) {
        boost += Math.min(10, h.uses);
        break;
      }
    }
  }

  return boost;
}

/** Chips synthesized purely from habits (always available). */
export function learnedChipsFromMemory(
  memory: QuickAssistMemory,
  now = Date.now(),
): QuickChip[] {
  return memory.hits
    .filter((h) => h.uses >= 1 && h.value.trim().length > 0)
    .filter((h) => h.kind === "chat" || h.kind === "shell")
    .map((h) => {
      const ageDays = (now - h.lastUsedAt) / 86400_000;
      const score =
        25 +
        Math.min(45, Math.log2(1 + h.uses) * 10) +
        Math.max(0, 15 - ageDays * 2) +
        (h.typedUses > 0 ? 5 : 0);
      return {
        id: `learn-${h.key.slice(0, 40)}`,
        label: h.label,
        value: h.value,
        kind: h.kind,
        score,
        hint: h.uses >= 5 ? "habit" : "learned",
      } satisfies QuickChip;
    })
    .sort((a, b) => b.score - a.score)
    .slice(0, 8);
}

/** Apply memory boosts to a chip list and inject learned favorites. */
export function applyMemoryToChips(
  chips: QuickChip[],
  memory: QuickAssistMemory | null | undefined,
): QuickChip[] {
  if (!memory?.hits?.length) return chips;
  const now = Date.now();
  const boosted = chips.map((c) => ({
    ...c,
    score: c.score + memoryBoostForChip(memory, c, now),
  }));
  const learned = learnedChipsFromMemory(memory, now);
  // Merge — prefer higher score for same value
  const byVal = new Map<string, QuickChip>();
  for (const c of [...boosted, ...learned]) {
    const k = c.value.trim().toLowerCase();
    const prev = byVal.get(k);
    if (!prev || c.score > prev.score) byVal.set(k, c);
  }
  return Array.from(byVal.values());
}

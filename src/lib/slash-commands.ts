/** Slash commands for autocomplete + /help alignment */

export type SlashDef = {
  cmd: string;
  hint: string;
  insert?: string; // what to put in composer; default cmd + space
  /** If true, Tab/Enter inserts then parent can auto-send when no trailing space needed */
  runOnPick?: boolean;
};

export const SLASH_COMMANDS: SlashDef[] = [
  { cmd: "/help", hint: "Show slash commands", runOnPick: true },
  { cmd: "/new", hint: "New chat", runOnPick: true },
  { cmd: "/clear", hint: "Clear current chat", runOnPick: true },
  { cmd: "/compact", hint: "Compact older turns", runOnPick: true },
  { cmd: "/context", hint: "Show context budget", runOnPick: true },
  { cmd: "/health", hint: "Run install/session health pass", runOnPick: true },
  { cmd: "/fix", hint: "Self-heal stuck UI + health pass", runOnPick: true },
  { cmd: "/memory", hint: "Show memory files", insert: "/memory ", runOnPick: false },
  { cmd: "/memory show", hint: "Show USER + MEMORY + today", runOnPick: true },
  { cmd: "/learn", hint: "Learning stats", runOnPick: true },
  { cmd: "/learn reflect", hint: "Run self-improve reflect", runOnPick: true },
  { cmd: "/mode", hint: "Set mode…", insert: "/mode " },
  { cmd: "/mode auto", hint: "Adaptive routing", runOnPick: true },
  { cmd: "/mode adaptive", hint: "Alias for auto", runOnPick: true },
  { cmd: "/mode fast", hint: "Fast replies", runOnPick: true },
  { cmd: "/mode balanced", hint: "Everyday balanced", runOnPick: true },
  { cmd: "/mode expert", hint: "Think / reasoning", runOnPick: true },
  { cmd: "/mode think", hint: "Alias → expert", runOnPick: true },
  { cmd: "/mode heavy", hint: "Heavy / complex", runOnPick: true },
  { cmd: "/mode max", hint: "Top-tier flagship", runOnPick: true },
  { cmd: "/mode deep", hint: "Alias → max", runOnPick: true },
  { cmd: "/mode build", hint: "Coding / build", runOnPick: true },
  { cmd: "/imagine", hint: "Open Imagine", insert: "/imagine " },
  { cmd: "/export", hint: "Export chat markdown", runOnPick: true },
  { cmd: "/stop", hint: "Stop generation", runOnPick: true },
  { cmd: "/tools on", hint: "Enable host tools", runOnPick: true },
  { cmd: "/tools off", hint: "Disable host tools", runOnPick: true },
  { cmd: "/sh", hint: "Run shell on host", insert: "/sh " },
  { cmd: "$", hint: "Host shell shortcut", insert: "$ " },
];

export function filterSlashCommands(draft: string): SlashDef[] {
  const t = draft.trimStart();
  if (!t.startsWith("/") && !t.startsWith("$")) return [];
  // multi-word: keep filtering while second token incomplete
  const parts = t.split(/\s+/);
  if (parts.length > 2) return [];
  const needle = t.toLowerCase();
  return SLASH_COMMANDS.filter((s) => {
    const c = s.cmd.toLowerCase();
    if (c.startsWith(needle)) return true;
    if (needle.startsWith(c + " ")) return true;
    // partial second token for /mode f…
    if (parts.length === 2 && c.startsWith(parts[0]!.toLowerCase() + " " + parts[1]!.toLowerCase()))
      return true;
    return false;
  }).slice(0, 12);
}

/** Map mode aliases used in /mode */
export function resolveModeArg(arg: string): string | null {
  const a = arg.trim().toLowerCase();
  const map: Record<string, string> = {
    auto: "auto",
    adaptive: "auto",
    smart: "auto",
    fast: "fast",
    balanced: "balanced",
    expert: "expert",
    think: "expert",
    thinking: "expert",
    heavy: "heavy",
    max: "max",
    deep: "max",
    build: "build",
  };
  return map[a] || null;
}

export function slashHelpMarkdown(): string {
  const lines = ["**Slash commands**", ""];
  for (const s of SLASH_COMMANDS) {
    if (s.cmd === "$") continue;
    lines.push(`- \`${s.cmd}\` — ${s.hint}`);
  }
  lines.push("", "Type `/` in the composer for autocomplete · **Tab** or **Enter** to accept.");
  return lines.join("\n");
}

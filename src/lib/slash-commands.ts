/** Slash commands for autocomplete + /help alignment */

export type SlashDef = {
  cmd: string;
  hint: string;
  insert?: string; // what to put in composer; default cmd + space
};

export const SLASH_COMMANDS: SlashDef[] = [
  { cmd: "/help", hint: "Show slash commands" },
  { cmd: "/clear", hint: "Clear current chat" },
  { cmd: "/compact", hint: "Compact older turns" },
  { cmd: "/context", hint: "Show context budget" },
  { cmd: "/memory", hint: "Open memory notes" },
  { cmd: "/mode", hint: "Set mode", insert: "/mode " },
  { cmd: "/mode adaptive", hint: "Adaptive routing" },
  { cmd: "/mode fast", hint: "Fast replies" },
  { cmd: "/mode think", hint: "Deeper reasoning" },
  { cmd: "/mode max", hint: "Top-tier Grok 4.5" },
  { cmd: "/mode deep", hint: "Max depth" },
  { cmd: "/mode build", hint: "Coding / build" },
  { cmd: "/imagine", hint: "Jump to Imagine", insert: "/imagine " },
  { cmd: "/new", hint: "New chat" },
  { cmd: "/stop", hint: "Stop generation" },
  { cmd: "/export", hint: "Export chat markdown" },
  { cmd: "/sh", hint: "Run shell on host", insert: "/sh " },
  { cmd: "$", hint: "Host shell shortcut", insert: "$ " },
];

export function filterSlashCommands(draft: string): SlashDef[] {
  const t = draft.trimStart();
  if (!t.startsWith("/") && !t.startsWith("$")) return [];
  // only while typing the first token
  if (/\s/.test(t.slice(1)) && !t.startsWith("/mode")) {
    // allow /mode <partial>
    if (!t.startsWith("/mode")) return [];
  }
  const needle = t.toLowerCase();
  return SLASH_COMMANDS.filter((s) => {
    if (s.cmd.startsWith(needle) || needle.startsWith(s.cmd.split(" ")[0]!)) return true;
    return s.cmd.toLowerCase().includes(needle.replace(/^\//, ""));
  }).slice(0, 10);
}

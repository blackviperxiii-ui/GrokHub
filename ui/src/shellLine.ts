/** A shell line sent to the agent (`/sh`, `$ ` and the Command page) goes as
 * the message `grokhub_core::shell_line::prompt` builds; the chat shows the
 * line itself. This reads that message back, the same way the engine does
 * for a reopened chat. */
const ASK =
  "Run this shell command in the project folder, exactly as written and once, " +
  "then show me what it printed and its exit code. Don't change it or run " +
  "anything else.";

function fenceFor(command: string): string {
  const runs = command.match(/`+/g) ?? [];
  const longest = Math.max(0, ...runs.map((r) => r.length));
  return "`".repeat(Math.max(longest + 1, 3));
}

/** The command in a shell line's message, or `null` for any other text. */
export function shellCommandOf(text: string): string | null {
  if (!text.startsWith(`${ASK}\n\n`)) return null;
  const body = text.slice(ASK.length + 2);
  const fence = /^`+/.exec(body)?.[0] ?? "";
  if (fence.length < 3) return null;
  const open = `${fence}sh\n`;
  const close = `\n${fence}`;
  if (body.length < open.length + close.length || !body.startsWith(open) || !body.endsWith(close)) return null;
  const command = body.slice(open.length, body.length - close.length);
  if (/[\n\r]/.test(command) || fenceFor(command) !== fence) return null;
  return command;
}

/** What the chat shows for a message: `$ <line>` for a shell line. */
export function shownText(text: string): string {
  const command = shellCommandOf(text);
  return command === null ? text : `$ ${command}`;
}

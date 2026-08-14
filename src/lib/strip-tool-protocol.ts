/** Drop model-only tool protocol from text shown to the user. */

const PROTOCOL_LINE =
  /^\s*(?:HOST_CMD|COMPUTER_CMD|CONNECTOR_CMD|SELF_MOD_CMD|HOST_RESULT|COMPUTER_RESULT|CONNECTOR_RESULT|SELF_MOD_RESULT|WORK_PIN|WORK_UPDATE|MEMORY_NOTE|LEARN_NOTE)\b/i;

const SHELL_ECHO = /^\s*\$\s+\S/;
const EXIT_LINE = /^\s*exit\s+\d+\b/;

export function stripToolProtocolForUser(text: string): string {
  const out = String(text || "")
    .split("\n")
    .filter((line) => !PROTOCOL_LINE.test(line) && !SHELL_ECHO.test(line) && !EXIT_LINE.test(line))
    .join("\n");
  return out.replace(/\n{3,}/g, "\n\n").trim();
}

/** Keep a single assistant turn in the tool-loop history when several tools run. */

export function appendAssistantOnce<T extends { role: string; content?: string }>(
  history: T[],
  content: string,
): void {
  for (let i = history.length - 1; i >= 0; i--) {
    const m = history[i];
    if (m?.role === "assistant") {
      if (m.content === content) return;
      break;
    }
  }
  history.push({ role: "assistant", content } as T);
}

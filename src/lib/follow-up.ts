/** Queue a follow-up while a turn is already running. */

export const MAX_FOLLOW_UPS = 2;

export function enqueueFollowUp(queue: string[], text: string): string[] {
  const t = String(text || "").trim();
  if (!t) return queue;
  if (queue.some((q) => q === t)) return queue;
  return [...queue, t].slice(-MAX_FOLLOW_UPS);
}

export function takeFollowUp(queue: string[]): { next: string | null; rest: string[] } {
  if (!queue.length) return { next: null, rest: [] };
  return { next: queue[0]!, rest: queue.slice(1) };
}

export function shouldStopOnSubmit(busy: boolean, composerText: string): boolean {
  return busy && !String(composerText || "").trim();
}

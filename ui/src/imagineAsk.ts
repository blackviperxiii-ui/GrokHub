import type { ImagineKind } from "./api";

/** A `/imagine` or `/imagine-video` the chat just ran. Imagine applies it
 * when the page is open, and on the next open if it is not mounted yet. */
export interface ImagineJob {
  kind: ImagineKind;
  prompt: string;
  token: number;
}

let job: ImagineJob | null = null;
let token = 0;
const listeners = new Set<(job: ImagineJob) => void>();

export function requestImagine(kind: ImagineKind, prompt: string): ImagineJob {
  token += 1;
  job = { kind, prompt, token };
  const current = job;
  listeners.forEach((fn) => fn(current));
  return current;
}

/** Calls `fn` for the job already waiting, and for each one after. */
export function subscribeImagine(fn: (job: ImagineJob) => void): () => void {
  listeners.add(fn);
  if (job) fn(job);
  return () => {
    listeners.delete(fn);
  };
}

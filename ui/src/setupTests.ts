import "@testing-library/jest-dom/vitest";
// vmThreads contexts don't carry Node's web streams; the web transport reads
// its event stream through one. (Imported by name: the UI has no Node types.)
if (typeof globalThis.ReadableStream === "undefined") {
  const streams = "node:stream/web";
  const { ReadableStream } = (await import(/* @vite-ignore */ streams)) as { ReadableStream: unknown };
  Object.assign(globalThis, { ReadableStream });
}
// jsdom draws nothing on a canvas and logs a warning for each try; the orb
// (Orb.tsx) checks for a missing context and simply doesn't paint.
HTMLCanvasElement.prototype.getContext = (() => null) as typeof HTMLCanvasElement.prototype.getContext;

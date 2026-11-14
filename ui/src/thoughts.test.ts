import { describe, expect, it } from "vitest";
import type { Item } from "./Chat";
import { collapseAll, expandOne, onlyFoldsChanged } from "./thoughts";

const log: Item[] = [
  { kind: "user", text: "why?" },
  { kind: "thought", text: "hmm" },
  { kind: "assistant", text: "because", streaming: false },
];

describe("onlyFoldsChanged", () => {
  it("is true when a fold is the only change", () => {
    expect(onlyFoldsChanged(log, collapseAll(log))).toBe(true);
    expect(onlyFoldsChanged(collapseAll(log), expandOne(collapseAll(log), 1))).toBe(true);
  });

  it("is false when a row is added, so the log follows the new row", () => {
    expect(onlyFoldsChanged(log, [...log, { kind: "assistant", text: "more", streaming: false }])).toBe(false);
  });

  it("is false when a thought or reply grows in place", () => {
    const grown = log.map((item) => (item.kind === "thought" ? { ...item, text: "hmm, well" } : item));
    expect(onlyFoldsChanged(log, grown)).toBe(false);
    const streamed = log.map((item) => (item.kind === "assistant" ? { ...item, text: "because so" } : item));
    expect(onlyFoldsChanged(log, streamed)).toBe(false);
  });
});

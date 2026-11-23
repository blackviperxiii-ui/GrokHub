import { describe, expect, it } from "vitest";
import type { Item } from "./Chat";
import { scrubHeld } from "./heldScrub";

const TOKEN = "sk-abcdefghijklmnopqrstuv";

describe("scrubHeld", () => {
  it("redacts a newly held value from the model's text, thoughts and cards", () => {
    const items: Item[] = [
      { kind: "user", text: `my key is ${TOKEN}` },
      { kind: "thought", text: `They gave ${TOKEN}.` },
      { kind: "tool", id: "t1", title: "Run", status: "done", detail: `curl -H "Bearer ${TOKEN}"` },
      {
        kind: "permission",
        id: "p1",
        title: "Run a command",
        action: `export KEY=${TOKEN}`,
        reason: "",
        answer: null,
      },
      { kind: "assistant", text: `Saved ${TOKEN} and ${TOKEN}.`, streaming: true },
    ];
    expect(scrubHeld(items, ` ${TOKEN} `)).toEqual([
      { kind: "user", text: `my key is ${TOKEN}` },
      { kind: "thought", text: "They gave [redacted]." },
      { kind: "tool", id: "t1", title: "Run", status: "done", detail: 'curl -H "Bearer [redacted]"' },
      {
        kind: "permission",
        id: "p1",
        title: "Run a command",
        action: "export KEY=[redacted]",
        reason: "",
        answer: null,
      },
      { kind: "assistant", text: "Saved [redacted] and [redacted].", streaming: true },
    ]);
  });

  it("keeps the same items when nothing holds the value or it is too short", () => {
    const items: Item[] = [{ kind: "assistant", text: "the code is abc", streaming: false }];
    expect(scrubHeld(items, TOKEN)).toBe(items);
    expect(scrubHeld(items, "abc")).toBe(items);
    expect(scrubHeld(items, "abc")[0]).toEqual({ kind: "assistant", text: "the code is abc", streaming: false });
  });
});

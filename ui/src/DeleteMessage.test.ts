import { describe, expect, it } from "vitest";
import { cutFor, withoutCut } from "./DeleteMessage";

const log = [
  { kind: "assistant", text: "Welcome back." },
  { kind: "user", text: "hi" },
  { kind: "tools", text: "read_file" },
  { kind: "assistant", text: "Hello." },
  { kind: "notice", text: "" },
  { kind: "user", text: " hi " },
  { kind: "assistant", text: "Again." },
];

describe("Delete message", () => {
  it("an ask goes alone, counting the same ask sent again after it", () => {
    expect(cutFor(log, 1, 0)).toEqual({ from: 1, to: 2, reply: false, text: "hi", later: 1 });
    expect(cutFor(log, 5, 0)).toEqual({ from: 5, to: 6, reply: false, text: " hi ", later: 0 });
  });

  it("a reply goes whole up to the next ask, but run notices stay", () => {
    const cut = cutFor(log, 3, 0);
    expect(cut).toEqual({ from: 2, to: 5, reply: true, text: "hi", later: 1 });
    expect(withoutCut(log, cut!).map((item) => item.text)).toEqual(["Welcome back.", "hi", "", " hi ", "Again."]);
  });

  it("a reply before any ask has none, unless its ask is still above Show earlier", () => {
    expect(cutFor(log, 0, 0)).toEqual({ from: 0, to: 1, reply: true, text: null, later: 0 });
    expect(cutFor(log, 0, 4)).toBeNull();
    expect(cutFor(log, 4, 0)).toBeNull();
    expect(cutFor(log, 99, 0)).toBeNull();
  });
});

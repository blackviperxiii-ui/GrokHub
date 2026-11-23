import { describe, expect, it } from "vitest";
import { findLabel, findRows, findStep, itemText } from "./chatFind";

// The literal cases of grokhub_core::chat_view's chat_find tests.
describe("Find in this chat", () => {
  it("finds rows ignoring case, and a blank query finds nothing", () => {
    const rows = ["Fix the Build", "nothing here", "build again", "BUILD"];
    expect(findRows(rows, "build")).toEqual([0, 2, 3]);
    expect(findRows(rows, "  ")).toEqual([]);
    expect(findRows(rows, "zzz")).toEqual([]);
    expect(findRows(["Émile"], "émile")).toEqual([0]);
  });

  it("steps wrap at both ends and the label counts from one", () => {
    expect(findStep(0, 3, true)).toBe(1);
    expect(findStep(2, 3, true)).toBe(0);
    expect(findStep(0, 3, false)).toBe(2);
    expect(findStep(9, 3, false)).toBe(1);
    expect(findStep(0, 0, true)).toBe(0);
    expect(findLabel(1, 5, "x")).toBe("2 of 5");
    expect(findLabel(0, 0, "x")).toBe("No matches");
    expect(findLabel(0, 0, " ")).toBe("");
  });

  it("searches what a panel shows, and nothing once it is dismissed", () => {
    const spec = {
      title: "Linux CI",
      elements: [
        { type: "heading" as const, level: 2, text: "Checks" },
        { type: "list" as const, ordered: false, items: ["fmt", "clippy"] },
        { type: "table" as const, headers: ["Job", "Result"], rows: [["nextest", "failed"]] },
        { type: "key_value" as const, pairs: [{ key: "Branch", value: "beta" }] },
        { type: "button" as const, label: "Re-run", action: "rerun_ci" },
        { type: "input" as const, id: "b", label: "Commit", value: "14b181df" },
        { type: "progress" as const, label: "Tests", value: 0.856 },
      ],
    };
    expect(itemText({ kind: "panel", id: "call-1", spec })).toBe(
      "Linux CI\nChecks\nfmt\nclippy\nJob Result\nnextest failed\nBranch: beta\nRe-run\nCommit: 14b181df\nTests — 86%",
    );
    expect(itemText({ kind: "panel", id: "call-1", spec, dismissed: true })).toBe("");
  });

  it("searches what a row says, and rows with no words find nothing", () => {
    expect(itemText({ kind: "user", text: "build it" })).toBe("build it");
    expect(itemText({ kind: "thought", text: "check the port first", fold: "minimized" })).toBe("check the port first");
    expect(itemText({ kind: "tool", id: "t", title: "read_file", status: "done", detail: "src/lib.rs" })).toBe(
      "read_file\nsrc/lib.rs",
    );
    expect(
      itemText({ kind: "permission", id: "p", title: "write", action: "note.txt", reason: "", answer: null }),
    ).toBe("write\nnote.txt");
    expect(itemText({ kind: "halted" })).toBe("");
  });
});

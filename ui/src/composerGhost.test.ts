import { describe, expect, it } from "vitest";
import {
  acceptSuggestion,
  cursorAtEnd,
  draftKey,
  MIN_DRAFT_CHARS,
  normalizeSuggestion,
  shouldRequest,
} from "./composerGhost";

describe("composerGhost", () => {
  it("requests only when the draft is long enough, not a slash, and idle", () => {
    expect(shouldRequest("hello there", false, false)).toBe(true);
    expect(shouldRequest("", false, false)).toBe(false);
    expect(shouldRequest("  ", false, false)).toBe(false);
    expect(shouldRequest("/help", false, false)).toBe(false);
    expect(shouldRequest("ab", false, false)).toBe(false);
    expect("ab".length < MIN_DRAFT_CHARS).toBe(true);
    expect(shouldRequest("hello", true, false)).toBe(false);
    expect(shouldRequest("hello", false, true)).toBe(false);
  });

  it("treats the cursor as at the end only past the last character", () => {
    expect(cursorAtEnd("hello", 5)).toBe(true);
    expect(cursorAtEnd("hello", 4)).toBe(false);
    expect(cursorAtEnd("café", 4)).toBe(true);
  });

  it("normalizes a reply that echoes the draft or wraps quotes", () => {
    expect(normalizeSuggestion('" world"', "hello")).toBe("world");
    expect(normalizeSuggestion("hello world", "hello")).toBe("world");
    expect(normalizeSuggestion("  world\nmore", "hello")).toBe("world");
    expect(normalizeSuggestion("", "hello")).toBe("");
  });

  it("accept appends the suggestion without changing prior characters", () => {
    expect(acceptSuggestion("hello", " world")).toBe("hello world");
    expect(acceptSuggestion("hello", "")).toBe("hello");
  });

  it("draft keys change when the draft or cursor moves", () => {
    expect(draftKey("hi", 2)).toBe(draftKey("hi", 2));
    expect(draftKey("hi", 2)).not.toBe(draftKey("hi", 1));
    expect(draftKey("hi", 2)).not.toBe(draftKey("ho", 2));
  });
});

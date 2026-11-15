import { describe, expect, it } from "vitest";
import { claimDesk, deskLook, deskSpeaker, releaseDesk } from "./nyxLife";

describe("deskLook", () => {
  it("follows the voice first, then a reply running here", () => {
    expect(deskLook("listening", false)).toBe("listening");
    expect(deskLook("listening", true)).toBe("listening");
    expect(deskLook("speaking", true)).toBe("speaking");
    expect(deskLook("thinking", false)).toBe("thinking");
    expect(deskLook("off", true)).toBe("thinking");
    expect(deskLook("off", false)).toBe("idle");
  });
});

describe("the chat that speaks for her desk", () => {
  it("is the one that claimed it last, until it goes away", () => {
    const a = Symbol("a");
    const b = Symbol("b");
    claimDesk(a);
    expect([deskSpeaker(a), deskSpeaker(b)]).toEqual([true, false]);
    claimDesk(b);
    expect([deskSpeaker(a), deskSpeaker(b)]).toEqual([false, true]);
    // A chat that isn't speaking going away changes nothing.
    releaseDesk(a);
    expect(deskSpeaker(b)).toBe(true);
    releaseDesk(b);
    expect(deskSpeaker(b)).toBe(false);
  });
});

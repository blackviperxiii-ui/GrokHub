import { describe, expect, it } from "vitest";
import { deskLook } from "./nyxLife";

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

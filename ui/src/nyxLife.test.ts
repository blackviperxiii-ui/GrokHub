import { beforeEach, describe, expect, it, vi } from "vitest";

const api = vi.hoisted(() => ({ desktopNyxLook: vi.fn<(look: string, activity: string) => Promise<void>>() }));
vi.mock("./desktopNyxApi", () => api);

import { deskLook, deskSpeaker, leaveDesk, updateDesk } from "./nyxLife";

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
  const sent: string[] = [];
  const chat = (name: string) => {
    const me = Symbol(name);
    return {
      me,
      update: (onScreen: boolean, voiced = false) => updateDesk(me, () => sent.push(name), { onScreen, voiced }),
      leave: () => leaveDesk(me),
    };
  };
  beforeEach(() => {
    sent.length = 0;
    api.desktopNyxLook.mockReset().mockResolvedValue();
  });

  it("is the chat last on screen, which keeps it while no chat is on screen", () => {
    const a = chat("a");
    const b = chat("b");
    a.update(true);
    b.update(false);
    expect(deskSpeaker()).toBe(a.me);
    expect(sent).toEqual(["a"]);
    b.update(true);
    a.update(false);
    // Settings: neither on screen, b still speaks.
    b.update(false);
    expect(deskSpeaker()).toBe(b.me);
    expect(sent).toEqual(["a", "b", "b"]);
    a.leave();
    b.leave();
    expect(deskSpeaker()).toBe(null);
  });

  it("goes to a live voice over the chat on screen, and back when the voice stops", () => {
    const main = chat("main");
    const code = chat("code");
    main.update(true, true);
    // The Code tab comes on screen while the main chat's voice talks on.
    code.update(true);
    main.update(false, true);
    expect(deskSpeaker()).toBe(main.me);
    expect(sent).toEqual(["main", "main"]);
    // The voice stops: the chat on screen sends its own look.
    main.update(false, false);
    expect(deskSpeaker()).toBe(code.me);
    expect(sent).toEqual(["main", "main", "code"]);
    main.leave();
    code.leave();
  });

  it("hands her desk on when its chat goes away, and settles her when none is left", () => {
    const a = chat("a");
    const b = chat("b");
    a.update(false);
    b.update(true, true);
    b.update(true, false);
    a.update(true);
    expect(sent).toEqual(["b", "b", "a"]);
    // b isn't speaking: its going changes nothing.
    b.leave();
    expect(sent).toEqual(["b", "b", "a"]);
    expect(api.desktopNyxLook).not.toHaveBeenCalled();
    a.leave();
    expect(deskSpeaker()).toBe(null);
    expect(api.desktopNyxLook.mock.calls).toEqual([["idle", "hanging-out"]]);
  });

  it("hands her desk from a voice that goes away to the chat last on screen", () => {
    const a = chat("a");
    const b = chat("b");
    a.update(true);
    b.update(false, true);
    expect(sent).toEqual(["a", "b"]);
    b.leave();
    expect(deskSpeaker()).toBe(a.me);
    expect(sent).toEqual(["a", "b", "a"]);
    expect(api.desktopNyxLook).not.toHaveBeenCalled();
    a.leave();
  });
});

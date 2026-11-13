import { describe, expect, it } from "vitest";
import {
  clearNotices,
  markRead,
  NO_NOTICES,
  NOTICE_CAP,
  noticeAge,
  pushNotice,
  shownText,
  spendAction,
  unreadBadge,
  chatFrom,
} from "./notices";

describe("the Notifications menu's model", () => {
  it("keeps the newest first and counts what is unread", () => {
    let n = pushNotice(NO_NOTICES, "success", "Digest finished", 1);
    n = pushNotice(n, "error", "board failed", 2);
    expect(n.items.map((i) => i.text)).toEqual(["board failed", "Digest finished"]);
    expect(n.unread).toBe(2);
    expect(markRead(n).unread).toBe(0);
    expect(markRead(n).items).toHaveLength(2);
  });

  it("folds the same words in a row into one entry", () => {
    let n = pushNotice(NO_NOTICES, "info", "Reply finished", 1);
    n = markRead(n);
    n = pushNotice(n, "info", "Reply finished", 5);
    expect(n.items).toHaveLength(1);
    expect(shownText(n.items[0])).toBe("Reply finished (x2)");
    expect(n.items[0].at).toBe(5);
    expect(n.unread).toBe(1);
    // Other words, or another button, start a new entry.
    n = pushNotice(n, "info", "Reply finished", 6, { kind: "chat", label: "Open chat" });
    expect(n.items).toHaveLength(2);
  });

  it("keeps at most the cap and clears to empty", () => {
    let n = NO_NOTICES;
    for (let i = 0; i < NOTICE_CAP + 5; i++) n = pushNotice(n, "info", `note ${i}`, i);
    expect(n.items).toHaveLength(NOTICE_CAP);
    expect(n.items[0].text).toBe(`note ${NOTICE_CAP + 4}`);
    expect(n.unread).toBe(NOTICE_CAP);
    const cleared = clearNotices(n);
    expect(cleared.items).toEqual([]);
    expect(cleared.unread).toBe(0);
  });

  it("a pressed Undo is spent but the entry stays; an Open button stays", () => {
    const n = pushNotice(NO_NOTICES, "info", "Archived 2 finished cards", 1, { kind: "undoSweep", label: "Undo" });
    const spent = spendAction(n, n.items[0].id);
    expect(spent.items[0].action).toBeUndefined();
    expect(spent.items[0].text).toBe("Archived 2 finished cards");
    const open = pushNotice(NO_NOTICES, "success", 'Reply finished in "Trip"', 1, {
      kind: "chat",
      label: "Open chat",
      id: "c1",
    });
    expect(spendAction(open, open.items[0].id).items[0].action).toEqual({ kind: "chat", label: "Open chat", id: "c1" });
  });

  it("an entry keeps where it came from, and the same words from another place don't fold", () => {
    let n = pushNotice(NO_NOTICES, "success", "Saved", 1, undefined, "Workboard");
    n = pushNotice(n, "success", "Saved", 2, undefined, chatFrom("Trip plans"));
    expect(n.items.map((i) => i.from)).toEqual(["Chat: Trip plans", "Workboard"]);
    expect(chatFrom("  ")).toBe("Chat");
  });

  it("the bell number hides at 0 and caps at 99+", () => {
    expect(unreadBadge(0)).toBe("");
    expect(unreadBadge(7)).toBe("7");
    expect(unreadBadge(99)).toBe("99");
    expect(unreadBadge(120)).toBe("99+");
  });

  it("says how long ago in plain words", () => {
    expect(noticeAge(1000, 30_000)).toBe("just now");
    expect(noticeAge(0, 5 * 60_000)).toBe("5 min ago");
    expect(noticeAge(0, 2 * 3_600_000)).toBe("2 h ago");
  });
});

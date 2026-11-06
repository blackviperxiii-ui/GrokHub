import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { NoticeBell } from "./NoticeBell";
import { NO_NOTICES, pushNotice } from "./notices";

const notices = pushNotice(
  pushNotice(NO_NOTICES, "success", "Digest finished: 3 new posts", 0, {
    kind: "draft",
    label: "Ask about it",
    text: "About the Digest run: ",
  }),
  "error",
  "board failed: API key rejected",
  60_000,
);

describe("NoticeBell", () => {
  it("shows the unread number, and opening it reads them", async () => {
    const onOpen = vi.fn();
    render(<NoticeBell notices={notices} onOpen={onOpen} onClear={() => {}} onAction={() => {}} now={120_000} />);
    const bell = screen.getByRole("button", { name: "Notifications, 2 new" });
    expect(bell).toHaveTextContent("2");
    await userEvent.click(bell);
    expect(onOpen).toHaveBeenCalledTimes(1);
    const menu = screen.getByRole("region", { name: "Notifications" });
    const rows = within(menu).getAllByRole("listitem");
    expect(rows[0]).toHaveTextContent("board failed: API key rejected");
    expect(rows[0]).toHaveTextContent("1 min ago");
    expect(rows[1]).toHaveTextContent("Digest finished: 3 new posts");
  });

  it("runs a row's button and closes, and Clear all empties it", async () => {
    const onAction = vi.fn();
    const onClear = vi.fn();
    render(<NoticeBell notices={notices} onOpen={() => {}} onClear={onClear} onAction={onAction} now={120_000} />);
    await userEvent.click(screen.getByRole("button", { name: /Notifications/ }));
    await userEvent.click(screen.getByRole("button", { name: "Ask about it" }));
    expect(onAction).toHaveBeenCalledWith(expect.objectContaining({ text: "Digest finished: 3 new posts" }));
    expect(screen.queryByRole("region", { name: "Notifications" })).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: /Notifications/ }));
    await userEvent.click(screen.getByRole("button", { name: "Clear all" }));
    expect(onClear).toHaveBeenCalledTimes(1);
  });

  it("an empty bell has no number and says what lands there", async () => {
    render(<NoticeBell notices={NO_NOTICES} onOpen={() => {}} onClear={() => {}} onAction={() => {}} />);
    const bell = screen.getByRole("button", { name: "Notifications" });
    expect(bell).not.toHaveTextContent(/\d/);
    await userEvent.click(bell);
    expect(screen.getByRole("region", { name: "Notifications" })).toHaveTextContent("Nothing new.");
    expect(screen.queryByRole("button", { name: "Clear all" })).toBeNull();
  });
});

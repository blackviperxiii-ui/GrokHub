import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { HALT_DONE, HALT_TITLE, HaltButton, HaltNote, haltShown, isHaltKey } from "./Halt";

function key(init: KeyboardEventInit): KeyboardEvent {
  return new KeyboardEvent("keydown", init);
}

describe("Halt", () => {
  it("Ctrl+Alt+H is the key, by its place on the keyboard", () => {
    expect(isHaltKey(key({ ctrlKey: true, altKey: true, code: "KeyH", key: "h" }))).toBe(true);
    // macOS Option turns the character into ˙; the key's place still says H.
    expect(isHaltKey(key({ ctrlKey: true, altKey: true, code: "KeyH", key: "˙" }))).toBe(true);
    expect(isHaltKey(key({ ctrlKey: true, code: "KeyH", key: "h" }))).toBe(false);
    expect(isHaltKey(key({ altKey: true, code: "KeyH", key: "h" }))).toBe(false);
    expect(isHaltKey(key({ ctrlKey: true, altKey: true, shiftKey: true, code: "KeyH", key: "H" }))).toBe(false);
    expect(isHaltKey(key({ ctrlKey: true, altKey: true, code: "KeyK", key: "k" }))).toBe(false);
  });

  it("shows while desktop control is on or anything runs", () => {
    expect(haltShown(false, false)).toBe(false);
    expect(haltShown(true, false)).toBe(true);
    expect(haltShown(false, true)).toBe(true);
  });

  it("the button names its keys and halts on a press", async () => {
    const onHalt = vi.fn();
    render(<HaltButton onHalt={onHalt} />);
    const button = screen.getByRole("button", { name: "Halt" });
    expect(button).toHaveAttribute("title", HALT_TITLE);
    expect(HALT_TITLE).toBe("Stop desktop control and every reply now (Ctrl+Alt+H)");
    await userEvent.click(button);
    expect(onHalt).toHaveBeenCalledTimes(1);
  });

  it("the note says desktop steps stay off until a restart, and can be dismissed", async () => {
    const onDismiss = vi.fn();
    render(<HaltNote text={HALT_DONE} onDismiss={onDismiss} />);
    expect(screen.getByRole("status")).toHaveTextContent(
      "Halted. Desktop steps stay off until you restart Loadout.",
    );
    await userEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });
});

import "@testing-library/jest-dom/vitest";
import { useState, type ReactNode } from "react";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { Capabilities, FixView } from "./api";
import type { AutoReply } from "./autoSetupApi";
import type { OnboardAct, OnboardReply } from "./onboardApi";

const FIX: FixView = {
  title: "GPU ran hot",
  verdict: null,
  options: [{ kind: "ask", title: "Ask about it", why: "Talk it through.", steps: [] }],
  compare: [],
};
vi.mock("./api", () => ({ pulseFix: () => Promise.resolve(FIX) }));
vi.mock("./autoSetupApi", () => ({ autoSetup: () => Promise.resolve(null), TEXT_STEPS: [100, 115, 130, 150] }));
vi.mock("./onboardApi", async (orig) => {
  const real = await orig<typeof import("./onboardApi")>();
  const reply: OnboardReply = {
    pending: true,
    stage: "pending",
    step: "welcome",
    answers: real.EMPTY_ANSWERS,
    drafts: null,
    amr: false,
    openclaw: false,
    saved: null,
    note: null,
  };
  return {
    ...real,
    onboard: (act: OnboardAct) => Promise.resolve({ ...reply, pending: act.act !== "skip" }),
  };
});

import { FirstRun } from "./AutoSetup";
import { ClearChatDialog } from "./ClearChat";
import { DeleteMessageDialog } from "./DeleteMessage";
import { Lightbox } from "./FeedCard";
import { OnboardStage } from "./GetToKnowYou";
import { Palette } from "./Palette";
import { FixWindow } from "./PulseFix";
import { Shortcuts } from "./Shortcuts";

const CAPS: Capabilities = {
  desktopControl: false,
  pulse: false,
  localAi: false,
  tray: false,
  webAccess: false,
  plugins: false,
  library: false,
  autoSetup: false,
};
const WELCOME: AutoReply = {
  firstRun: true,
  textSize: 100,
  reduceMotion: false,
  theme: "system",
  question: null,
  summary: false,
  made: [],
  offered: [],
  note: null,
  rows: [],
  said: null,
};

/** A page with a button that opens the window, and one more behind it. */
function Host({ dialog }: { dialog: (close: () => void) => ReactNode }) {
  const [open, setOpen] = useState(false);
  return (
    <div>
      <button type="button">before</button>
      <button type="button" onClick={() => setOpen(true)}>
        opener
      </button>
      {open && dialog(() => setOpen(false))}
      <button type="button">behind</button>
    </div>
  );
}

/** Each window, with the names of its first and last control. */
const WINDOWS: [string, (close: () => void) => ReactNode, string, string][] = [
  ["Clear chat", (c) => <ClearChatDialog onConfirm={c} onClose={c} />, "Clear chat", "Keep"],
  ["Delete message", (c) => <DeleteMessageDialog reply={false} onConfirm={c} onClose={c} />, "Delete", "Keep"],
  ["Image", (c) => <Lightbox src="data:image/png;base64,AA==" name="a.png" onClose={c} />, "Download", "Close"],
  ["PC health options", (c) => <FixWindow id="gpu-hot" label="GPU ran hot" onAsk={() => {}} onClose={c} />, "Close", "Ask"],
  ["Command palette", (c) => <Palette caps={CAPS} onPick={() => {}} onClose={c} />, "Search pages and commands", "Close"],
  ["Keyboard shortcuts", (c) => <Shortcuts keys={[{ keys: "Ctrl+K", action: "Command palette" }]} onClose={c} />, "Close", "Close"],
  ["First run", (c) => <FirstRun reply={WELCOME} act={(a) => a.act === "finish" && c()} />, "Make things bigger", "Just set it up"],
  ["Get to know you", () => <OnboardStage after />, "Skip for now", "Start"],
];

describe("a window over the page", () => {
  it.each(WINDOWS)("%s keeps Tab inside, closes on Escape and gives focus back", async (_name, dialog, first, last) => {
    render(<Host dialog={dialog} />);
    const opener = screen.getByRole("button", { name: "opener" });
    await userEvent.click(opener);
    const box = await waitFor(() => {
      const found = document.querySelector<HTMLElement>('[aria-modal="true"]');
      expect(found).not.toBeNull();
      return found as HTMLElement;
    });
    await screen.findAllByRole(first === "Search pages and commands" ? "combobox" : first === "Download" ? "link" : "button", { name: first });
    await screen.findAllByRole("button", { name: last });

    for (let i = 0; i < 6; i++) {
      await userEvent.tab();
      expect(box).toContainElement(document.activeElement as HTMLElement);
    }
    for (let i = 0; i < 6; i++) {
      await userEvent.tab({ shift: true });
      expect(box).toContainElement(document.activeElement as HTMLElement);
    }

    const controls = box.querySelectorAll<HTMLElement>("a[href], button, input");
    const firstControl = controls[0];
    const lastControl = controls[controls.length - 1];
    expect(firstControl).toHaveAccessibleName(first);
    expect(lastControl).toHaveAccessibleName(last);
    lastControl.focus();
    await userEvent.tab();
    expect(firstControl).toHaveFocus();
    await userEvent.tab({ shift: true });
    expect(lastControl).toHaveFocus();

    await userEvent.keyboard("{Escape}");
    await waitFor(() => expect(document.querySelector('[aria-modal="true"]')).toBeNull());
    expect(opener).toHaveFocus();
  });

  it("the window opened last holds the keyboard when two are open", async () => {
    const stay = () => {};
    function Two() {
      const [palette, setPalette] = useState(false);
      return (
        <>
          <ClearChatDialog onConfirm={() => setPalette(true)} onClose={stay} />
          {palette && <Palette caps={CAPS} onPick={() => {}} onClose={() => setPalette(false)} />}
        </>
      );
    }
    render(<Two />);
    await userEvent.click(screen.getByRole("button", { name: "Clear chat" }));
    const palette = screen.getByRole("dialog", { name: "Command palette" });
    const search = screen.getByRole("combobox", { name: "Search pages and commands" });
    expect(search).toHaveFocus();
    await userEvent.tab();
    expect(palette).toContainElement(document.activeElement as HTMLElement);
    expect(document.activeElement).toHaveAccessibleName("Close");
    await userEvent.tab();
    expect(search).toHaveFocus();
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("dialog", { name: "Command palette" })).toBeNull();
    expect(screen.getByRole("button", { name: "Clear chat" })).toHaveFocus();
    await userEvent.tab();
    expect(screen.getByRole("button", { name: "Keep" })).toHaveFocus();
  });
});

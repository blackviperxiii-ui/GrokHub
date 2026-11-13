import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Act, AutoReply } from "./autoSetupApi";

const mock = vi.hoisted(() => ({
  autoSetup: vi.fn<(act: Act) => Promise<AutoReply | null>>(),
}));
vi.mock("./autoSetupApi", () => ({ autoSetup: mock.autoSetup, TEXT_STEPS: [100, 115, 130, 150] }));

import { AutoNote, AutoSetupPage, FirstRun, LookRows, SUMMARY } from "./AutoSetup";

const BASE: AutoReply = {
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

describe("FirstRun", () => {
  it("welcomes with a big text button and two ways to start", async () => {
    const act = vi.fn();
    render(<FirstRun reply={{ ...BASE, textSize: 115 }} act={act} />);
    expect(screen.getByRole("dialog", { name: "Welcome to Loadout" })).toBeInTheDocument();
    expect(screen.getByText("Text size: 115%")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Make things bigger" }));
    expect(act).toHaveBeenLastCalledWith({ act: "bigger" });
    await userEvent.click(screen.getByRole("button", { name: "Just set it up" }));
    expect(act).toHaveBeenLastCalledWith({ act: "start", pace: "quick" });
    // Waiting on the first question.
    expect(screen.getByRole("dialog", { name: "Checking this PC…" })).toBeInTheDocument();
  });

  it("asks one question at a time, and Skip and Esc move on", async () => {
    const act = vi.fn();
    const question = {
      id: "talk",
      prompt: "How should I talk to you?",
      choices: [
        { label: "Friendly", value: "friendly" },
        { label: "Straight to the point", value: "direct" },
      ],
    };
    render(<FirstRun reply={{ ...BASE, question }} act={act} />);
    await userEvent.click(screen.getByRole("button", { name: "Straight to the point" }));
    expect(act).toHaveBeenLastCalledWith({ act: "answer", id: "talk", value: "direct" });
    await userEvent.click(screen.getByRole("button", { name: "Skip this one" }));
    expect(act).toHaveBeenLastCalledWith({ act: "answer", id: "talk" });
    await userEvent.keyboard("{Escape}");
    expect(act).toHaveBeenLastCalledWith({ act: "finish" });
  });

  it("sums up what it set, with Undo, and what it offers, with Yes", async () => {
    const act = vi.fn();
    render(
      <FirstRun
        reply={{
          ...BASE,
          summary: true,
          made: [{ key: "textSize", text: "Text size: 150%", reason: "Windows shows text at 150%." }],
          offered: [{ key: "closeToTray", text: "Turn off keep running in the tray?", reason: "This PC runs on battery." }],
        }}
        act={act}
      />,
    );
    expect(screen.getByText(SUMMARY)).toBeInTheDocument();
    expect(screen.getByText("Windows shows text at 150%.")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Undo: Text size: 150%" }));
    expect(act).toHaveBeenLastCalledWith({ act: "undo", key: "textSize" });
    await userEvent.click(screen.getByRole("button", { name: "Yes: Turn off keep running in the tray?" }));
    expect(act).toHaveBeenLastCalledWith({ act: "accept", key: "closeToTray" });
    await userEvent.click(screen.getByRole("button", { name: "Done" }));
    expect(act).toHaveBeenLastCalledWith({ act: "finish" });
  });
});

describe("AutoNote", () => {
  it("a change offers Undo, an offer offers Yes and Not now", async () => {
    const act = vi.fn();
    const close = vi.fn();
    const line = { key: "textSize" as const, text: "Text size: 130%", reason: "Your screen is small." };
    const { rerender } = render(<AutoNote note={{ line, offer: false }} act={act} onClose={close} />);
    expect(screen.getByRole("status")).toHaveTextContent("Text size: 130%Your screen is small.");
    await userEvent.click(screen.getByRole("button", { name: "Undo" }));
    expect(act).toHaveBeenLastCalledWith({ act: "undo", key: "textSize" });
    expect(close).toHaveBeenCalledTimes(1);
    rerender(<AutoNote note={{ line, offer: true }} act={act} onClose={close} />);
    await userEvent.click(screen.getByRole("button", { name: "Not now" }));
    expect(act).toHaveBeenLastCalledWith({ act: "dismiss", key: "textSize" });
    await userEvent.click(screen.getByRole("button", { name: "Yes" }));
    expect(act).toHaveBeenLastCalledWith({ act: "accept", key: "textSize" });
  });
});

describe("Settings pages", () => {
  beforeEach(() => mock.autoSetup.mockReset());

  it("lists what auto setup set, with Undo or Hand back", async () => {
    const reply: AutoReply = {
      ...BASE,
      firstRun: false,
      rows: [
        { key: "textSize", label: "Text size", text: "150%. Windows shows text at 150%.", undo: true },
        { key: "theme", label: "Theme", text: "Dark. You set this.", undo: false },
      ],
    };
    mock.autoSetup.mockResolvedValue(reply);
    render(<AutoSetupPage />);
    expect(await screen.findByText("150%. Windows shows text at 150%.")).toBeInTheDocument();
    expect(mock.autoSetup).toHaveBeenCalledWith({ act: "view" });
    await userEvent.click(screen.getByRole("button", { name: "Undo Text size" }));
    expect(mock.autoSetup).toHaveBeenLastCalledWith({ act: "undo", key: "textSize" });
    await userEvent.click(screen.getByRole("button", { name: "Hand back Theme" }));
    expect(mock.autoSetup).toHaveBeenLastCalledWith({ act: "handBack", key: "theme" });
    expect(await screen.findByText("Theme is Loadout's to tune again.")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Forget what you learned" }));
    expect(mock.autoSetup).toHaveBeenLastCalledWith({ act: "forget" });
  });

  it("Appearance picks a text size and reduce motion as the person's own", async () => {
    mock.autoSetup.mockResolvedValue({ ...BASE, firstRun: false, textSize: 115 });
    render(<LookRows />);
    const size = await screen.findByRole("button", { name: "115%" });
    expect(size).toHaveAttribute("aria-pressed", "true");
    await userEvent.click(screen.getByRole("button", { name: "130%" }));
    expect(mock.autoSetup).toHaveBeenLastCalledWith({ act: "set", key: "textSize", value: { t: "pct", v: 130 } });
    await userEvent.click(screen.getByRole("switch", { name: "Reduce motion" }));
    await waitFor(() =>
      expect(mock.autoSetup).toHaveBeenLastCalledWith({
        act: "set",
        key: "reduceMotion",
        value: { t: "bool", v: true },
      }),
    );
  });
});

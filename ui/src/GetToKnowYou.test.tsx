import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Drafts, OnboardAct, OnboardReply } from "./onboardApi";

const mock = vi.hoisted(() => ({
  onboard: vi.fn<(act: OnboardAct) => Promise<OnboardReply | null>>(),
}));
vi.mock("./onboardApi", async (orig) => ({
  ...(await orig<typeof import("./onboardApi")>()),
  onboard: mock.onboard,
}));

import { EMPTY_ANSWERS, OPEN_EVENT } from "./onboardApi";
import { OnboardStage, WELCOME_TITLE } from "./GetToKnowYou";

const BASE: OnboardReply = {
  pending: true,
  stage: "pending",
  step: "welcome",
  answers: { ...EMPTY_ANSWERS, tone: "direct" },
  drafts: null,
  amr: true,
  openclaw: false,
  saved: null,
  note: null,
};

const DRAFTS: Drafts = {
  user: "# User\n\n- Call me: Sam\n",
  soul: "# Soul\n\n- Tone: direct. Short answers, the point first, no filler.\n",
  memory: "# Memory\n\n- 2026-01-02: Goals: Finish a 10k run\n",
  rows: [
    { group: "about", text: "Call me Sam" },
    { group: "needs", text: "Goals: Finish a 10k run" },
  ],
};

/** Answers each act the way the shell would, with `over` on every reply. */
function shell(over: Partial<OnboardReply> = {}) {
  mock.onboard.mockImplementation(async (act) => {
    switch (act.act) {
      case "draft":
        return {
          ...BASE,
          ...over,
          step: "review",
          answers: act.answers,
          drafts: DRAFTS,
        };
      case "keep":
        return { ...BASE, ...over, step: act.step, answers: act.answers };
      case "save":
        return {
          ...BASE,
          ...over,
          stage: "done",
          step: "done",
          saved: {
            files: ["USER.md", "SOUL.md", "MEMORY.md"],
            skipped: [],
            rows: over.amr === false ? 0 : 2,
            amr: over.amr ?? true,
          },
        };
      case "skip":
        return { ...BASE, ...over, pending: false, stage: "skipped" };
      default:
        return { ...BASE, ...over };
    }
  });
}

function acts(): string[] {
  return mock.onboard.mock.calls.map(([a]) => a.act);
}

beforeEach(() => {
  mock.onboard.mockReset();
  shell();
});

describe("OnboardStage", () => {
  it("waits while Auto Setup's first run is open, then shows after it", async () => {
    const { rerender } = render(<OnboardStage after={false} />);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(mock.onboard).not.toHaveBeenCalled();
    rerender(<OnboardStage after />);
    expect(await screen.findByRole("dialog", { name: WELCOME_TITLE })).toBeInTheDocument();
    expect(WELCOME_TITLE).toBe("Help Loadout get to know you");
    expect(mock.onboard).toHaveBeenCalledWith({ act: "view" });
  });

  it("stays closed when the shell says it is done or skipped", async () => {
    shell({ pending: false, stage: "done" });
    render(<OnboardStage after />);
    await waitFor(() => expect(mock.onboard).toHaveBeenCalledWith({ act: "view" }));
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("walks the steps, reviews the exact drafts and saves them with the Personal rows", async () => {
    render(<OnboardStage after />);
    await userEvent.click(await screen.findByRole("button", { name: "Start" }));
    expect(mock.onboard).toHaveBeenLastCalledWith({
      act: "keep",
      step: "about",
      answers: BASE.answers,
    });
    await userEvent.type(screen.getByLabelText("What should I call you?"), "Sam");
    await userEvent.click(screen.getByRole("button", { name: "Next" }));
    // The soul step starts from Auto Setup's Talk answer.
    expect(screen.getByRole("button", { name: "Straight to the point" })).toHaveAttribute("aria-pressed", "true");
    await userEvent.click(screen.getByRole("button", { name: "Next" }));
    await userEvent.type(screen.getByLabelText("Goals you're working on"), "Finish a 10k run");
    await userEvent.click(screen.getByRole("button", { name: "Review" }));
    expect(mock.onboard).toHaveBeenLastCalledWith({
      act: "draft",
      answers: { ...BASE.answers, callMe: "Sam", goals: "Finish a 10k run" },
    });
    expect(await screen.findByRole("dialog", { name: "Check what I'll save" })).toBeInTheDocument();
    expect(screen.getByLabelText("About you (USER.md)")).toHaveValue("# User\n\n- Call me: Sam\n");
    expect(screen.getByLabelText("Notes to keep (MEMORY.md)")).toHaveValue(DRAFTS.memory);
    const rows = screen.getByRole("list", { name: "Personal rows" });
    expect(rows).toHaveTextContent("Call me Sam");
    expect(rows).toHaveTextContent("What you need");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(mock.onboard).toHaveBeenLastCalledWith({
      act: "save",
      drafts: DRAFTS,
    });
    expect(
      await screen.findByText(
        "Saved USER.md, SOUL.md, MEMORY.md and 2 Personal rows. Edit any of it later in Settings → Personal.",
      ),
    ).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Done" }));
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("an edit or a removed row on Review is what Save sends", async () => {
    shell({ step: "review" });
    render(<OnboardStage after />);
    const user = await screen.findByLabelText("About you (USER.md)");
    await userEvent.clear(user);
    await userEvent.type(user, "# User");
    await userEvent.click(screen.getByRole("button", { name: "Remove: Call me Sam" }));
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(mock.onboard).toHaveBeenLastCalledWith({
      act: "save",
      drafts: { ...DRAFTS, user: "# User", rows: [DRAFTS.rows[1]] },
    });
  });

  it("with AMR off, Review lists no Personal rows", async () => {
    shell({ step: "review", amr: false });
    render(<OnboardStage after />);
    expect(await screen.findByLabelText("About you (USER.md)")).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "Personal rows" })).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText(/^Saved USER\.md, SOUL\.md, MEMORY\.md\. /)).toBeInTheDocument();
  });

  it("offers no chat-export or account import, and OpenClaw only when one is here", async () => {
    shell({ step: "review" });
    const { unmount } = render(<OnboardStage after />);
    expect(await screen.findByLabelText("About you (USER.md)")).toBeInTheDocument();
    for (const text of [/chat export/i, /export/i, /connected account/i, /sign in/i, /google|gmail|outlook/i]) {
      expect(screen.queryByText(text)).toBeNull();
    }
    expect(screen.queryByRole("button", { name: "Add from OpenClaw" })).toBeNull();
    unmount();
    shell({ step: "review", openclaw: true });
    render(<OnboardStage after />);
    await userEvent.click(await screen.findByRole("button", { name: "Add from OpenClaw" }));
    expect(mock.onboard).toHaveBeenLastCalledWith({
      act: "openclaw",
      drafts: DRAFTS,
    });
    expect(screen.getAllByRole("button").map((b) => b.textContent)).toEqual([
      "Remove",
      "Remove",
      "Add from OpenClaw",
      "Back",
      "Skip for now",
      "Save",
    ]);
  });

  it("resumes at the step it was left on, with what was typed", async () => {
    shell({ step: "life", answers: { ...EMPTY_ANSWERS, interests: "chess" } });
    render(<OnboardStage after />);
    expect(await screen.findByRole("dialog", { name: "Your life and work" })).toBeInTheDocument();
    expect(screen.getByLabelText("Interests")).toHaveValue("chess");
  });

  it("Skip for now and Escape skip it; Settings opens it again", async () => {
    render(<OnboardStage after />);
    await userEvent.click(await screen.findByRole("button", { name: "Skip for now" }));
    expect(acts()).toEqual(["view", "skip"]);
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    shell();
    window.dispatchEvent(new Event(OPEN_EVENT));
    expect(await screen.findByRole("dialog", { name: WELCOME_TITLE })).toBeInTheDocument();
    expect(mock.onboard).toHaveBeenLastCalledWith({ act: "reopen" });
    await userEvent.keyboard("{Escape}");
    expect(mock.onboard).toHaveBeenLastCalledWith({ act: "skip" });
  });

  it("/import opens Review with the OpenClaw notes added and saves nothing", async () => {
    const withNotes: Drafts = { ...DRAFTS, user: `${DRAFTS.user}\n## From OpenClaw USER.md\n\nLikes tea.\n` };
    shell({ openclaw: true });
    mock.onboard.mockImplementation(async (act) => {
      if (act.act === "openclaw") return { ...BASE, openclaw: true, step: "review", drafts: withNotes };
      if (act.act === "draft") return { ...BASE, openclaw: true, step: "review", answers: act.answers, drafts: DRAFTS };
      return { ...BASE, openclaw: true };
    });
    render(<OnboardStage after />);
    await waitFor(() => expect(acts()).toContain("view"));
    window.dispatchEvent(new CustomEvent(OPEN_EVENT, { detail: { import: true } }));
    expect(await screen.findByLabelText("About you (USER.md)")).toHaveValue(withNotes.user);
    expect(mock.onboard).toHaveBeenCalledWith({ act: "openclaw", drafts: DRAFTS });
    expect(acts()).not.toContain("save");
    await userEvent.click(screen.getByRole("button", { name: "Remove: Call me Sam" }));
    expect(screen.queryByRole("button", { name: "Remove: Call me Sam" })).toBeNull();
    expect(acts()).not.toContain("save");
  });
  describe("/import over a finished stage", () => {
    const DONE: OnboardReply = { ...BASE, pending: false, stage: "done", step: "done", answers: EMPTY_ANSWERS, openclaw: true };
    const withNotes: Drafts = { ...DRAFTS, user: `${DRAFTS.user}\n## From OpenClaw USER.md\n\nLikes tea.\n` };

    /** The shell after Get to know you was finished: nothing is pending. */
    function finished(over: Partial<Record<OnboardAct["act"], () => Promise<OnboardReply>>> = {}) {
      mock.onboard.mockImplementation(async (act) => {
        const own = over[act.act];
        if (own) return own();
        switch (act.act) {
          case "draft":
            return { ...DONE, drafts: DRAFTS };
          case "openclaw":
            return { ...DONE, drafts: withNotes };
          case "save":
            return {
              ...DONE,
              pending: true,
              saved: { files: ["USER.md"], skipped: [], rows: 0, amr: true },
            };
          default:
            return DONE;
        }
      });
    }

    async function open() {
      render(<OnboardStage after />);
      await waitFor(() => expect(acts()).toEqual(["view"]));
      expect(screen.queryByRole("dialog")).toBeNull();
      window.dispatchEvent(new CustomEvent(OPEN_EVENT, { detail: { import: true } }));
    }

    it("asks for the files as they are: the stage is not opened again and no answer is sent", async () => {
      finished();
      await open();
      await waitFor(() => expect(acts()).toContain("openclaw"));
      expect(acts()).toEqual(["view", "draft", "openclaw"]);
      expect(mock.onboard).toHaveBeenCalledWith({ act: "draft", answers: EMPTY_ANSWERS });
      expect(mock.onboard).toHaveBeenCalledWith({ act: "openclaw", drafts: DRAFTS });
      expect(await screen.findByRole("dialog", { name: "Check what I'll save" })).toBeInTheDocument();
      expect(screen.getByLabelText("About you (USER.md)")).toHaveValue(withNotes.user);
      // The notes are in, so nothing offers to add them again; there is no
      // step to go back to and nothing to skip.
      expect(screen.getAllByRole("button").map((b) => b.textContent)).toEqual(["Remove", "Remove", "Cancel", "Save"]);
    });

    it("Escape closes it and tells the shell nothing", async () => {
      finished();
      await open();
      expect(await screen.findByLabelText("About you (USER.md)")).toBeInTheDocument();
      await userEvent.keyboard("{Escape}");
      expect(screen.queryByRole("dialog")).toBeNull();
      expect(acts()).toEqual(["view", "draft", "openclaw"]);
    });

    it("Save shows what was saved, and Done closes it", async () => {
      finished();
      await open();
      await userEvent.click(await screen.findByRole("button", { name: "Save" }));
      expect(mock.onboard).toHaveBeenLastCalledWith({ act: "save", drafts: withNotes });
      expect(await screen.findByText("Saved USER.md. Edit any of it later in Settings → Personal.")).toBeInTheDocument();
      await userEvent.click(screen.getByRole("button", { name: "Done" }));
      expect(screen.queryByRole("dialog")).toBeNull();
    });

    it("shows the shell's note under the drafts", async () => {
      const note = "The OpenClaw notes are already in these files; nothing was added.";
      finished({ openclaw: async () => ({ ...DONE, drafts: DRAFTS, note }) });
      await open();
      expect(await screen.findByText(note)).toBeInTheDocument();
      expect(screen.queryByRole("button", { name: "Add from OpenClaw" })).toBeNull();
    });

    it("a failure says why in the window instead of showing nothing", async () => {
      finished({ draft: () => Promise.reject("Couldn't read your notes folder.") });
      await open();
      expect(await screen.findByText("Couldn't read your notes folder.")).toBeInTheDocument();
      expect(screen.getAllByRole("button").map((b) => b.textContent)).toEqual(["Cancel"]);
      await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
      expect(screen.queryByRole("dialog")).toBeNull();
      expect(acts()).toEqual(["view", "draft"]);
    });
  });

  it("Add from OpenClaw is not offered again once it added the notes", async () => {
    shell({ step: "review", openclaw: true });
    const withNotes: Drafts = { ...DRAFTS, soul: `${DRAFTS.soul}\n## From OpenClaw SOUL.md\n\nBe calm.\n` };
    const plain = mock.onboard.getMockImplementation()!;
    mock.onboard.mockImplementation(async (act) =>
      act.act === "openclaw" ? { ...BASE, step: "review", openclaw: true, drafts: withNotes } : plain(act),
    );
    render(<OnboardStage after />);
    await userEvent.click(await screen.findByRole("button", { name: "Add from OpenClaw" }));
    await waitFor(() => expect(screen.getByLabelText("How I sound (SOUL.md)")).toHaveValue(withNotes.soul));
    expect(screen.queryByRole("button", { name: "Add from OpenClaw" })).toBeNull();
    // Back and forward drafts again from the answers, without the notes.
    await userEvent.click(screen.getByRole("button", { name: "Back" }));
    await userEvent.click(screen.getByRole("button", { name: "Review" }));
    expect(await screen.findByRole("button", { name: "Add from OpenClaw" })).toBeInTheDocument();
    expect(screen.getByLabelText("How I sound (SOUL.md)")).toHaveValue(DRAFTS.soul);
  });

  it("Save pressed again while it is saving sends one save", async () => {
    shell({ step: "review" });
    const plain = mock.onboard.getMockImplementation()!;
    let finish = () => {};
    mock.onboard.mockImplementation((act) =>
      act.act === "save"
        ? new Promise((done) => {
            finish = () => void plain(act).then(done);
          })
        : plain(act),
    );
    render(<OnboardStage after />);
    const save = await screen.findByRole("button", { name: "Save" });
    await userEvent.click(save);
    expect(save).toBeDisabled();
    await userEvent.click(save);
    expect(acts().filter((a) => a === "save")).toEqual(["save"]);
    finish();
    expect(
      await screen.findByText("Saved USER.md, SOUL.md, MEMORY.md and 2 Personal rows. Edit any of it later in Settings → Personal."),
    ).toBeInTheDocument();
  });

  it("a file too big to change is named on the Saved screen", async () => {
    shell({ step: "review" });
    const plain = mock.onboard.getMockImplementation()!;
    const note = "MEMORY.md is too big to change here; it was left alone.";
    mock.onboard.mockImplementation(async (act) =>
      act.act === "save"
        ? {
            ...BASE,
            stage: "done",
            step: "done",
            saved: { files: ["USER.md"], skipped: ["MEMORY.md"], rows: 0, amr: true },
            note,
          }
        : plain(act),
    );
    render(<OnboardStage after />);
    await userEvent.click(await screen.findByRole("button", { name: "Save" }));
    expect(await screen.findByText("Saved USER.md. Edit any of it later in Settings → Personal.")).toBeInTheDocument();
    expect(screen.getByText(note)).toBeInTheDocument();
  });
});

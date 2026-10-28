import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { PersonalityEdit, PersonalityPage, Preset } from "./api";

const api = vi.hoisted(() => ({
  getPersonalities: vi.fn<() => Promise<PersonalityPage>>(),
  setPersonality: vi.fn<(id: string) => Promise<PersonalityPage>>(),
  setExplicitPersonalities: vi.fn<(on: boolean) => Promise<PersonalityPage>>(),
  duplicatePersonality:
    vi.fn<(id: string) => Promise<[string, PersonalityPage]>>(),
  savePersonality: vi.fn<(edit: PersonalityEdit) => Promise<PersonalityPage>>(),
  deletePersonality: vi.fn<(id: string) => Promise<PersonalityPage>>(),
}));

vi.mock("./api", () => api);

import { ADULTS_ONLY, PERSONALITY_NOTE, Personality } from "./Personality";

function preset(over: Partial<Preset> = {}): Preset {
  return {
    id: "default",
    name: "Default",
    description: "GrokHub's own voice: brief, warm and plain.",
    sample: "",
    toneRules: "",
    emoji: "none",
    verbosity: "normal",
    explicit: false,
    custom: false,
    ...over,
  };
}

const PIRATE = preset({
  id: "pirate",
  name: "Pirate",
  description: "Arr.",
  sample: "Yer build sank, matey.",
  toneRules: "Talk like a pirate.",
  emoji: "light",
});
const COPY = {
  ...PIRATE,
  id: "pirate-copy",
  name: "Pirate copy",
  custom: true,
};

function page(
  current: string,
  presets: Preset[],
  problems: string[] = [],
  explicit = false,
): PersonalityPage {
  return { current, presets, problems, explicit };
}

const VIPER = preset({
  id: "viper",
  name: "Viper",
  description: "Vulgar.",
  toneRules: "Swear.",
  explicit: true,
});

function card(name: string) {
  const list = within(screen.getByRole("list", { name: "Personalities" }));
  return within(list.getByRole("listitem", { name }));
}

describe("Personality", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
  });

  it("turns adults-only personalities on and shows the explicit presets", async () => {
    api.getPersonalities.mockResolvedValue(page("default", [preset(), PIRATE]));
    api.setExplicitPersonalities.mockResolvedValue(
      page("default", [preset(), PIRATE, VIPER], [], true),
    );
    render(<Personality />);
    const toggle = await screen.findByRole("switch", { name: ADULTS_ONLY });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(screen.queryByRole("listitem", { name: "Viper" })).toBeNull();

    await userEvent.click(toggle);
    expect(api.setExplicitPersonalities).toHaveBeenCalledWith(true);
    expect(await screen.findByRole("status")).toHaveTextContent(
      "Adults-only personalities on.",
    );
    expect(toggle).toHaveAttribute("aria-checked", "true");
    expect(card("Viper").getByText("Explicit")).toBeVisible();
  });

  it("marks the default in use and makes another the default", async () => {
    api.getPersonalities.mockResolvedValue(page("default", [preset(), PIRATE]));
    api.setPersonality.mockResolvedValue(page("pirate", [preset(), PIRATE]));
    render(<Personality />);
    expect(await screen.findByText(PERSONALITY_NOTE)).toBeVisible();
    expect(card("Default").getByText("In use")).toBeVisible();
    expect(card("Default").queryByRole("button", { name: "Use" })).toBeNull();
    expect(card("Pirate").getByText("“Yer build sank, matey.”")).toBeVisible();
    expect(card("Pirate").queryByRole("button", { name: "Edit" })).toBeNull();

    await userEvent.click(card("Pirate").getByRole("button", { name: "Use" }));
    expect(api.setPersonality).toHaveBeenCalledWith("pirate");
    expect(card("Pirate").getByText("In use")).toBeVisible();
    expect(card("Default").getByText("Built in")).toBeVisible();
    expect(screen.getByRole("status")).toHaveTextContent("Personality: Pirate");
  });

  it("duplicates a built-in into the editor, saves, and deletes after a second click", async () => {
    api.getPersonalities.mockResolvedValue(page("default", [preset(), PIRATE]));
    api.duplicatePersonality.mockResolvedValue([
      "pirate-copy",
      page("default", [preset(), PIRATE, COPY]),
    ]);
    const deck = {
      ...COPY,
      name: "Deckhand",
      toneRules: "Talk like a deckhand.",
      verbosity: "terse" as const,
    };
    api.savePersonality.mockResolvedValue(
      page("default", [preset(), PIRATE, deck]),
    );
    api.deletePersonality.mockResolvedValue(
      page("default", [preset(), PIRATE]),
    );
    render(<Personality />);

    await screen.findByRole("list", { name: "Personalities" });
    await userEvent.click(card("Pirate").getByRole("button", { name: "Duplicate" }));
    expect(api.duplicatePersonality).toHaveBeenCalledWith("pirate");
    const editor = within(
      screen.getByRole("form", { name: "Edit Pirate copy" }),
    );
    expect(screen.getByRole("status")).toHaveTextContent(
      "Made Pirate copy. Change it below.",
    );
    expect(card("Pirate copy").getByText("Yours")).toBeVisible();

    const name = editor.getByLabelText("Name");
    await userEvent.clear(name);
    await userEvent.type(name, "Deckhand");
    const rules = editor.getByLabelText(/^Tone rules/);
    await userEvent.clear(rules);
    await userEvent.type(rules, "Talk like a deckhand.");
    expect(
      editor.getByText("Tone rules (21 of 600 characters)", { exact: false }),
    ).toBeVisible();
    await userEvent.selectOptions(editor.getByLabelText("Length"), "terse");
    await userEvent.click(editor.getByRole("button", { name: "Save" }));
    expect(api.savePersonality).toHaveBeenCalledWith({
      id: "pirate-copy",
      name: "Deckhand",
      description: "Arr.",
      toneRules: "Talk like a deckhand.",
      emoji: "light",
      verbosity: "terse",
    });
    expect(screen.getByRole("status")).toHaveTextContent("Saved Deckhand.");

    await userEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(api.deletePersonality).not.toHaveBeenCalled();
    await userEvent.click(
      screen.getByRole("button", { name: "Really delete" }),
    );
    expect(api.deletePersonality).toHaveBeenCalledWith("pirate-copy");
    expect(screen.queryByRole("form")).toBeNull();
    expect(screen.getByRole("status")).toHaveTextContent("Deleted Deckhand.");
  });

  it("shows a refused save and the files it skipped", async () => {
    api.getPersonalities.mockResolvedValue(
      page(
        "default",
        [preset(), COPY],
        ["bad.toml: Personalities: expected `=`"],
      ),
    );
    api.savePersonality.mockRejectedValue(
      "Personality pirate-copy: the name must be 1 to 32 characters.",
    );
    render(<Personality />);
    expect(
      await screen.findByText("Skipped bad.toml: Personalities: expected `=`"),
    ).toBeVisible();
    await userEvent.click(
      card("Pirate copy").getByRole("button", { name: "Edit" }),
    );
    const editor = within(
      screen.getByRole("form", { name: "Edit Pirate copy" }),
    );
    await userEvent.clear(editor.getByLabelText("Name"));
    await userEvent.click(editor.getByRole("button", { name: "Save" }));
    expect(screen.getByRole("status")).toHaveTextContent(
      "Personality pirate-copy: the name must be 1 to 32 characters.",
    );
    await userEvent.click(editor.getByRole("button", { name: "Reset" }));
    expect(editor.getByLabelText("Name")).toHaveValue("Pirate copy");
  });
});

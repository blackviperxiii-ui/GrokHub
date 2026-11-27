import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  SkillDetail,
  SkillLibraryPage,
  SkillPage,
  SkillRow,
  SkillSource,
  SkillsHubHit,
  SkillsHubInstall,
  SkillLibraryInstall,
  VstackStatus,
} from "./api";

const api = vi.hoisted(() => ({
  getSkills: vi.fn<() => Promise<SkillPage>>(),
  getSkillLibrary: vi.fn<() => Promise<SkillLibraryPage>>(),
  installLibrarySkill: vi.fn<(id: string) => Promise<SkillLibraryInstall>>(),
  installLibraryPack: vi.fn<(packId: string) => Promise<SkillLibraryInstall>>(),
  openSkillFolder:
    vi.fn<(source: SkillSource, name: string) => Promise<void>>(),
  setSkillOn: vi.fn<(name: string, on: boolean) => Promise<SkillPage>>(),
  createSkill: vi.fn<(name: string, what: string) => Promise<SkillPage>>(),
  getSkillDetail: vi.fn<(name: string) => Promise<SkillDetail>>(),
  editSkill: vi.fn<
    (
      name: string,
      description: string,
      trigger: string,
      instructions: string,
    ) => Promise<SkillPage>
  >(),
  searchSkillsHub: vi.fn<(query: string) => Promise<SkillsHubHit[]>>(),
  installSkillsHub: vi.fn<(hit: SkillsHubHit) => Promise<SkillsHubInstall>>(),
  vstackSyncStatus: vi.fn(),
  vstackSyncNow: vi.fn(),
}));

vi.mock("./api", () => api);

import {
  filterLibrarySkills,
  filterSkills,
  SKILL_GROUPS,
  SKILL_OFF_NOTE,
  Skills,
  SKILLS_EMPTY,
  SKILLS_HUB_NOTE,
  SKILLS_LIBRARY_NOTE,
  SKILLS_NONE_MATCHED,
} from "./Skills";

function row(over: Partial<SkillRow>): SkillRow {
  return {
    name: "tidy",
    description: "Tidy the repo",
    trigger: "Use the tidy skill.",
    source: "cabin",
    folder: "/home/user/.config/GrokHub/skills/tidy",
    on: true,
    ...over,
  };
}

const ROWS: SkillRow[] = [
  row({
    name: "deploy-site",
    description: "Ship the site",
    trigger: "deploy the site",
  }),
  row({
    name: "lint-fix",
    description: "Fix lint warnings",
    trigger: "Use the lint-fix skill.",
    source: "project",
  }),
  row({
    name: "qa-walkthrough",
    description: "Ship check",
    trigger: "Use the qa-walkthrough skill.",
    source: "plugin",
    folder: "",
  }),
];

function names() {
  return screen
    .queryAllByRole("listitem")
    .map((item) => item.getAttribute("aria-label"));
}

describe("filterSkills", () => {
  it("matches the name, the description or the trigger, ignoring case", () => {
    expect(filterSkills(ROWS, "LINT").map((r) => r.name)).toEqual(["lint-fix"]);
    expect(filterSkills(ROWS, "ship").map((r) => r.name)).toEqual([
      "deploy-site",
      "qa-walkthrough",
    ]);
    expect(filterSkills(ROWS, "the site").map((r) => r.name)).toEqual([
      "deploy-site",
    ]);
    expect(filterSkills(ROWS, "  ").map((r) => r.name)).toEqual([
      "deploy-site",
      "lint-fix",
      "qa-walkthrough",
    ]);
    expect(filterSkills(ROWS, "zzz")).toEqual([]);
  });
});

const VSTACK: VstackStatus = {
  repo: "https://github.com/blackviperxiii-ui/vstack",
  source: "bundled",
  version: "1.4.0",
  tag: "v1.4.0",
  commit: "11cca98b4a55da3118f51e749ac62faefbc21b3b",
  lastSyncMs: null,
  lastCheckMs: null,
  lastError: null,
  skillCount: 58,
};

describe("Skills", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
    api.vstackSyncStatus.mockResolvedValue(VSTACK);
    api.vstackSyncNow.mockResolvedValue(VSTACK);
  });

  it("groups skills by source and search filters by name, description and trigger", async () => {
    api.getSkills.mockResolvedValue({ rows: ROWS });
    render(<Skills onUse={() => {}} onOpenConnectors={() => {}} />);

    const cabin = await screen.findByRole("list", { name: "App skills" });
    expect(
      within(cabin).getByRole("listitem", { name: "deploy-site" }),
    ).toHaveTextContent("deploy the site");
    expect(
      within(screen.getByRole("list", { name: "Project skills" })).getByRole(
        "listitem",
      ),
    ).toHaveTextContent("lint-fix");
    expect(
      screen.getByRole("list", { name: "Plugin skills" }),
    ).toBeInTheDocument();
    expect(SKILL_GROUPS.map((g) => g.label)).toContain("Built-in (vstack)");
    expect(screen.queryByRole("list", { name: "User skills" })).toBeNull();

    const search = screen.getByLabelText("Search skills");
    await userEvent.type(search, "warnings");
    expect(names()).toEqual(["lint-fix"]);
    await userEvent.clear(search);
    await userEvent.type(search, "the site");
    expect(names()).toEqual(["deploy-site"]);
    await userEvent.clear(search);
    await userEvent.type(search, "QA-WALK");
    expect(names()).toEqual(["qa-walkthrough"]);
    await userEvent.type(search, "zzz");
    expect(names()).toEqual([]);
    expect(screen.getByText(SKILLS_NONE_MATCHED)).toBeInTheDocument();
  });

  it("lists the vstack library, keeps it read-only, and refreshes after Sync now", async () => {
    const builtins: SkillRow[] = [
      ...Array.from({ length: 68 }, (_, i) =>
        row({
          name: `skill-${String(i).padStart(2, "0")}`,
          description: "A vstack skill",
          trigger: "Use the skill.",
          source: "builtin",
          folder: "",
        }),
      ),
      row({
        name: "viper-mode",
        description: "Viper mode.",
        trigger: "Use the viper-mode skill.",
        source: "builtin",
        folder: "",
      }),
      row({
        name: "qa-walkthrough",
        description: "Ship check.",
        trigger: "Use the qa-walkthrough skill.",
        source: "builtin",
        folder: "",
      }),
      row({
        name: "arena",
        description: "Bake off designs.",
        trigger: "Use the arena skill.",
        source: "builtin",
        folder: "",
      }),
    ];
    const refreshed = builtins.map((item) =>
      item.name === "arena" ? { ...item, description: "Bake off designs. refreshed" } : item,
    );
    api.getSkills.mockResolvedValueOnce({ rows: builtins }).mockResolvedValue({ rows: refreshed });
    api.vstackSyncNow.mockResolvedValue({
      ...VSTACK,
      source: "synced",
      version: "9.9.9",
      tag: "v9.9.9",
      commit: "abcdef1234567890abcd",
      skillCount: 71,
    });
    render(<Skills onUse={() => {}} onOpenConnectors={() => {}} />);

    expect(await screen.findByText("bundled v1.4.0 · 11cca98b4a55 · 58 skills")).toBeInTheDocument();
    const list = screen.getByRole("list", { name: "Built-in (vstack)" });
    expect(within(list).getAllByRole("listitem")).toHaveLength(71);
    expect(within(list).getByRole("listitem", { name: "viper-mode" })).toHaveTextContent("Read-only.");
    expect(within(list).queryByRole("switch")).toBeNull();
    await userEvent.type(screen.getByLabelText("Search skills"), "arena");
    expect(names()).toEqual(["arena"]);
    await userEvent.click(screen.getByRole("button", { name: "Sync now" }));
    expect(api.vstackSyncNow).toHaveBeenCalledTimes(1);
    expect(await screen.findByText("Bake off designs. refreshed")).toBeInTheDocument();
    expect(screen.getByText("synced v9.9.9 · abcdef123456 · 71 skills")).toBeInTheDocument();
  });

  it("Use hands the skill's trigger to the chat box", async () => {
    api.getSkills.mockResolvedValue({ rows: ROWS });
    const onUse = vi.fn<(text: string) => void>();
    render(<Skills onUse={onUse} onOpenConnectors={() => {}} />);

    const deploy = await screen.findByRole("listitem", { name: "deploy-site" });
    await userEvent.click(within(deploy).getByRole("button", { name: "Use" }));
    expect(onUse).toHaveBeenCalledWith("deploy the site");
    await userEvent.click(
      within(screen.getByRole("listitem", { name: "lint-fix" })).getByRole(
        "button",
        { name: "Use" },
      ),
    );
    expect(onUse).toHaveBeenLastCalledWith("Use the lint-fix skill.");
  });

  it("Open folder names the skill, never a path, and a skill with no folder has none", async () => {
    api.getSkills.mockResolvedValue({ rows: ROWS });
    api.openSkillFolder.mockRejectedValue(
      "lint-fix isn't in the skills list any more.",
    );
    render(<Skills onUse={() => {}} onOpenConnectors={() => {}} />);

    const lint = await screen.findByRole("listitem", { name: "lint-fix" });
    await userEvent.click(
      within(lint).getByRole("button", { name: "Open folder" }),
    );
    expect(api.openSkillFolder).toHaveBeenCalledWith("project", "lint-fix");
    expect(await screen.findByRole("status")).toHaveTextContent(
      "lint-fix isn't in the skills list any more.",
    );
    const noFolder = screen.getByRole("listitem", { name: "qa-walkthrough" });
    expect(
      within(noFolder).queryByRole("button", { name: "Open folder" }),
    ).toBeNull();
  });

  it("a switch turns a skill off and back on, and an off skill has no Use", async () => {
    api.getSkills.mockResolvedValue({ rows: ROWS });
    const offRows = ROWS.map((r) =>
      r.name === "lint-fix" ? { ...r, on: false } : r,
    );
    api.setSkillOn
      .mockResolvedValueOnce({ rows: offRows })
      .mockResolvedValueOnce({ rows: ROWS });
    render(<Skills onUse={() => {}} onOpenConnectors={() => {}} />);

    const lint = await screen.findByRole("listitem", { name: "lint-fix" });
    await userEvent.click(
      within(lint).getByRole("switch", { name: "Chat can use it" }),
    );
    expect(api.setSkillOn).toHaveBeenLastCalledWith("lint-fix", false);
    const off = await screen.findByText(SKILL_OFF_NOTE);
    const offItem = screen.getByRole("listitem", { name: "lint-fix" });
    expect(offItem).toContainElement(off);
    expect(within(offItem).queryByRole("button", { name: "Use" })).toBeNull();
    expect(within(offItem).getByRole("switch")).toHaveAttribute(
      "aria-checked",
      "false",
    );

    await userEvent.click(
      within(offItem).getByRole("switch", { name: "Chat can use it" }),
    );
    expect(api.setSkillOn).toHaveBeenLastCalledWith("lint-fix", true);
    expect(
      await within(
        screen.getByRole("listitem", { name: "lint-fix" }),
      ).findByRole("button", { name: "Use" }),
    ).toBeInTheDocument();
    expect(screen.queryByText(SKILL_OFF_NOTE)).toBeNull();
  });

  it("the web app lists skills without Open folder or switches", async () => {
    api.getSkills.mockResolvedValue({ rows: ROWS });
    render(<Skills onUse={() => {}} onOpenConnectors={() => {}} remote />);

    await screen.findByRole("listitem", { name: "deploy-site" });
    expect(screen.queryByRole("button", { name: "Open folder" })).toBeNull();
    expect(screen.queryByRole("switch")).toBeNull();
    expect(screen.getAllByRole("button", { name: "Use" })).toHaveLength(3);
  });

  it("New skill saves a name and what it should do, then lists it", async () => {
    api.getSkills.mockResolvedValue({ rows: ROWS });
    api.createSkill.mockResolvedValue({
      rows: [
        ...ROWS,
        row({ name: "Clip Trimmer", description: "Trim my last game clip" }),
      ],
    });
    render(<Skills onUse={() => {}} onOpenConnectors={() => {}} />);

    await screen.findByRole("listitem", { name: "deploy-site" });
    await userEvent.click(screen.getByRole("button", { name: "New skill" }));
    const form = screen.getByRole("form", { name: "New skill" });
    await userEvent.type(within(form).getByLabelText("Name"), "Clip Trimmer");
    await userEvent.type(
      within(form).getByLabelText("What it should do"),
      "Trim my last game clip",
    );
    await userEvent.click(
      within(form).getByRole("button", { name: "Save skill" }),
    );
    expect(api.createSkill).toHaveBeenCalledWith(
      "Clip Trimmer",
      "Trim my last game clip",
    );
    expect(
      await screen.findByRole("listitem", { name: "Clip Trimmer" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("form", { name: "New skill" })).toBeNull();
  });

  it("a New skill the shell refuses stays open with the reason", async () => {
    api.getSkills.mockResolvedValue({ rows: ROWS });
    api.createSkill.mockRejectedValue("There's already a skill called tidy.");
    render(<Skills onUse={() => {}} onOpenConnectors={() => {}} />);

    await screen.findByRole("listitem", { name: "deploy-site" });
    await userEvent.click(screen.getByRole("button", { name: "New skill" }));
    const form = screen.getByRole("form", { name: "New skill" });
    await userEvent.type(within(form).getByLabelText("Name"), "tidy");
    await userEvent.type(
      within(form).getByLabelText("What it should do"),
      "Tidy again",
    );
    await userEvent.click(
      within(form).getByRole("button", { name: "Save skill" }),
    );
    expect(await screen.findByRole("status")).toHaveTextContent(
      "There's already a skill called tidy.",
    );
    expect(screen.getByRole("form", { name: "New skill" })).toBeInTheDocument();
  });

  it("Edit is a cabin-only button and saves the three fields", async () => {
    api.getSkills.mockResolvedValue({ rows: ROWS });
    api.getSkillDetail.mockResolvedValue({
      name: "deploy-site",
      description: "Ship the site",
      trigger: "deploy the site",
      instructions: "1. build\n2. ship",
    });
    const edited = ROWS.map((r) =>
      r.name === "deploy-site" ? { ...r, description: "Ship the whole site" } : r,
    );
    api.editSkill.mockResolvedValue({ rows: edited });
    render(<Skills onUse={() => {}} onOpenConnectors={() => {}} />);

    const deploy = await screen.findByRole("listitem", { name: "deploy-site" });
    const lint = screen.getByRole("listitem", { name: "lint-fix" });
    expect(within(lint).queryByRole("button", { name: "Edit" })).toBeNull();
    await userEvent.click(within(deploy).getByRole("button", { name: "Edit" }));
    expect(api.getSkillDetail).toHaveBeenCalledWith("deploy-site");

    const form = await screen.findByRole("form", { name: "Edit deploy-site" });
    expect(within(form).getByLabelText("What it does")).toHaveValue(
      "Ship the site",
    );
    expect(within(form).getByLabelText("When to use it")).toHaveValue(
      "deploy the site",
    );
    expect(within(form).getByLabelText("Steps")).toHaveValue("1. build\n2. ship");
    await userEvent.clear(within(form).getByLabelText("What it does"));
    await userEvent.type(
      within(form).getByLabelText("What it does"),
      "Ship the whole site",
    );
    await userEvent.click(
      within(form).getByRole("button", { name: "Save changes" }),
    );
    expect(api.editSkill).toHaveBeenCalledWith(
      "deploy-site",
      "Ship the whole site",
      "deploy the site",
      "1. build\n2. ship",
    );
    expect(
      await screen.findByRole("listitem", { name: "deploy-site" }),
    ).toHaveTextContent("Ship the whole site");
    expect(screen.queryByRole("form", { name: "Edit deploy-site" })).toBeNull();
  });

  it("the web app gets no New skill, no Edit, and no Skills Hub", async () => {
    api.getSkills.mockResolvedValue({ rows: ROWS });
    render(<Skills onUse={() => {}} onOpenConnectors={() => {}} remote />);

    await screen.findByRole("listitem", { name: "deploy-site" });
    expect(screen.queryByRole("button", { name: "New skill" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Edit" })).toBeNull();
    expect(screen.queryByLabelText("Skills Hub")).toBeNull();
  });

  it("Skills Hub search lists hits and Install refreshes the page", async () => {
    api.getSkills.mockResolvedValue({ rows: ROWS });
    api.searchSkillsHub.mockResolvedValue([
      {
        source: "clawhub",
        id: "find-skills",
        name: "find-skills",
        description: "Discover skills",
      },
    ]);
    const installed = row({
      name: "find-skills",
      description: "Discover skills",
      trigger: "Use the find-skills skill.",
    });
    api.installSkillsHub.mockResolvedValue({
      page: { rows: [...ROWS, installed] },
      note: "Installed find-skills. Chat can use it.",
    });
    render(<Skills onUse={() => {}} onOpenConnectors={() => {}} />);

    expect(await screen.findByText(SKILLS_HUB_NOTE)).toBeInTheDocument();
    await userEvent.type(
      screen.getByLabelText("Search Skills Hub"),
      "find skills",
    );
    await userEvent.click(screen.getByRole("button", { name: "Search Hub" }));
    const hub = await screen.findByRole("list", { name: "Skills Hub results" });
    expect(within(hub).getByRole("listitem", { name: "find-skills" })).toHaveTextContent(
      "clawhub",
    );
    await userEvent.click(
      within(hub).getByRole("button", { name: "Install" }),
    );
    expect(api.installSkillsHub).toHaveBeenCalledWith({
      source: "clawhub",
      id: "find-skills",
      name: "find-skills",
      description: "Discover skills",
    });
    expect(
      await within(
        await screen.findByRole("list", { name: "App skills" }),
      ).findByRole("listitem", { name: "find-skills" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "Skills Hub results" })).toBeNull();
    expect(
      screen.getByText("Installed find-skills. Chat can use it."),
    ).toBeInTheDocument();
  });

  it("says so when there are none, refreshes, and links to Connectors", async () => {
    api.getSkills
      .mockResolvedValueOnce({ rows: [] })
      .mockResolvedValueOnce({ rows: [ROWS[1]] });
    const onOpenConnectors = vi.fn();
    render(<Skills onUse={() => {}} onOpenConnectors={onOpenConnectors} />);

    expect(await screen.findByText(SKILLS_EMPTY)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(
      await screen.findByRole("listitem", { name: "lint-fix" }),
    ).toBeInTheDocument();
    expect(api.getSkills).toHaveBeenCalledTimes(2);
    await userEvent.click(screen.getByRole("button", { name: "Connectors" }));
    expect(onOpenConnectors).toHaveBeenCalledTimes(1);
  });

  it("Skills Library browses by category, previews steps, and installs single skills and the Everyday Life Pack", async () => {
    const libraryPage: SkillLibraryPage = {
      packs: [
        {
          id: "everyday-life",
          name: "Everyday Life Pack",
          description: "Twelve practical workflows.",
          category: "Everyday Life",
          total: 2,
          installed: 0,
        },
      ],
      categories: ["Finance & Shopping", "Home & Meals"],
      items: [
        {
          id: "weekly-meal-plan",
          name: "Weekly Meal Plan",
          description: "Build a 7-day meal plan and grouped grocery list.",
          category: "Home & Meals",
          tags: ["meals", "groceries"],
          pack: "everyday-life",
          trigger: "plan meals for the week",
          instructions: "1. Gather dietary needs.\n2. Group grocery list.",
          pitfalls: "Stick to everyday staples.",
          installed: false,
        },
        {
          id: "monthly-budget-check",
          name: "Monthly Budget Check",
          description: "Group expenses into fixed needs and savings targets.",
          category: "Finance & Shopping",
          tags: ["budget", "money"],
          pack: "everyday-life",
          trigger: "review my monthly budget and spending",
          instructions: "1. Summarize numbers.\n2. Highlight top cuts.",
          pitfalls: "No investment advice.",
          installed: false,
        },
      ],
    };

    expect(filterLibrarySkills(libraryPage.items, "Home & Meals", "").map((i) => i.id)).toEqual([
      "weekly-meal-plan",
    ]);
    expect(filterLibrarySkills(libraryPage.items, "", "budget").map((i) => i.id)).toEqual([
      "monthly-budget-check",
    ]);

    api.getSkills.mockResolvedValue({ rows: ROWS });
    api.getSkillLibrary.mockResolvedValue(libraryPage);

    const installedMealRow = row({
      name: "Weekly Meal Plan",
      description: "Build a 7-day meal plan and grouped grocery list.",
      trigger: "plan meals for the week",
      category: "Home & Meals",
      pack: "everyday-life",
    });
    const afterOneLib: SkillLibraryPage = {
      ...libraryPage,
      packs: [{ ...libraryPage.packs[0], installed: 1 }],
      items: [
        { ...libraryPage.items[0], installed: true },
        libraryPage.items[1],
      ],
    };
    api.installLibrarySkill.mockResolvedValue({
      page: { rows: [...ROWS, installedMealRow] },
      library: afterOneLib,
      note: "Installed Weekly Meal Plan. Chat can use it.",
    });

    const installedBudgetRow = row({
      name: "Monthly Budget Check",
      description: "Group expenses into fixed needs and savings targets.",
      trigger: "review my monthly budget and spending",
      category: "Finance & Shopping",
      pack: "everyday-life",
    });
    const afterPackLib: SkillLibraryPage = {
      ...libraryPage,
      packs: [{ ...libraryPage.packs[0], installed: 2 }],
      items: libraryPage.items.map((item) => ({ ...item, installed: true })),
    };
    api.installLibraryPack.mockResolvedValue({
      page: { rows: [...ROWS, installedMealRow, installedBudgetRow] },
      library: afterPackLib,
      note: "Installed 1 skills from Everyday Life Pack.",
    });

    render(<Skills onUse={() => {}} onOpenConnectors={() => {}} />);

    const lib = await screen.findByRole("region", { name: "Skills Library" });
    expect(within(lib).getByText(SKILLS_LIBRARY_NOTE)).toBeInTheDocument();
    expect(within(lib).getByText(/0 of 2 installed/)).toBeInTheDocument();

    // Filter by category pill
    await userEvent.click(within(lib).getByRole("button", { name: "Home & Meals" }));
    expect(within(lib).getByRole("article", { name: "Weekly Meal Plan" })).toBeInTheDocument();
    expect(within(lib).queryByRole("article", { name: "Monthly Budget Check" })).toBeNull();

    // Reset to All and search
    await userEvent.click(within(lib).getByRole("button", { name: "All" }));
    await userEvent.type(within(lib).getByLabelText("Search Skills Library"), "grocery");
    expect(within(lib).getByRole("article", { name: "Weekly Meal Plan" })).toBeInTheDocument();
    expect(within(lib).queryByRole("article", { name: "Monthly Budget Check" })).toBeNull();
    await userEvent.clear(within(lib).getByLabelText("Search Skills Library"));

    // Preview steps and pitfalls
    const mealCard = within(lib).getByRole("article", { name: "Weekly Meal Plan" });
    await userEvent.click(within(mealCard).getByRole("button", { name: "Preview" }));
    expect(within(mealCard).getByText(/Stick to everyday staples/)).toBeInTheDocument();

    // Install single skill
    await userEvent.click(within(mealCard).getByRole("button", { name: "Install" }));
    expect(api.installLibrarySkill).toHaveBeenCalledWith("weekly-meal-plan");
    expect(
      await screen.findByText("Installed Weekly Meal Plan. Chat can use it."),
    ).toBeInTheDocument();
    expect(within(lib).getByText(/1 of 2 installed/)).toBeInTheDocument();

    // Install rest of pack
    await userEvent.click(within(lib).getByRole("button", { name: "Install pack" }));
    expect(api.installLibraryPack).toHaveBeenCalledWith("everyday-life");
    expect(
      await within(lib).findByRole("button", { name: "Pack installed" }),
    ).toBeDisabled();
  });

  it("hides Skills Library in remote web mode", async () => {
    api.getSkills.mockResolvedValue({ rows: ROWS });
    render(<Skills onUse={() => {}} onOpenConnectors={() => {}} remote />);
    await screen.findByRole("listitem", { name: "deploy-site" });
    expect(screen.queryByRole("region", { name: "Skills Library" })).toBeNull();
    expect(api.getSkillLibrary).not.toHaveBeenCalled();
  });
});

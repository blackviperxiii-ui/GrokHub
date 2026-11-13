import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SkillDetail, SkillPage, SkillRow, SkillSource } from "./api";

const api = vi.hoisted(() => ({
  getSkills: vi.fn<() => Promise<SkillPage>>(),
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
}));

vi.mock("./api", () => api);

import {
  filterSkills,
  SKILL_GROUPS,
  SKILL_OFF_NOTE,
  Skills,
  SKILLS_EMPTY,
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

describe("Skills", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
  });

  it("groups skills by source and search filters by name, description and trigger", async () => {
    api.getSkills.mockResolvedValue({ rows: ROWS });
    render(<Skills onUse={() => {}} onOpenConnectors={() => {}} />);

    const cabin = await screen.findByRole("list", { name: "Cabin skills" });
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
    expect(SKILL_GROUPS.map((g) => g.source)).not.toContain("builtin");
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

  it("the web app gets no New skill and no Edit", async () => {
    api.getSkills.mockResolvedValue({ rows: ROWS });
    render(<Skills onUse={() => {}} onOpenConnectors={() => {}} remote />);

    await screen.findByRole("listitem", { name: "deploy-site" });
    expect(screen.queryByRole("button", { name: "New skill" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Edit" })).toBeNull();
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
});

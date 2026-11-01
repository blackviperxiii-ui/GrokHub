import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SkillPage, SkillRow, SkillSource } from "./api";

const api = vi.hoisted(() => ({
  getSkills: vi.fn<() => Promise<SkillPage>>(),
  openSkillFolder: vi.fn<(source: SkillSource, name: string) => Promise<void>>(),
}));

vi.mock("./api", () => api);

import { filterSkills, Skills, SKILLS_EMPTY, SKILLS_NONE_MATCHED } from "./Skills";

function row(over: Partial<SkillRow>): SkillRow {
  return {
    name: "tidy",
    description: "Tidy the repo",
    trigger: "Use the tidy skill.",
    source: "cabin",
    folder: "/home/user/.config/GrokHub/skills/tidy",
    ...over,
  };
}

const ROWS: SkillRow[] = [
  row({ name: "deploy-site", description: "Ship the site", trigger: "deploy the site" }),
  row({ name: "lint-fix", description: "Fix lint warnings", trigger: "Use the lint-fix skill.", source: "project" }),
  row({ name: "qa-walkthrough", description: "Ship check", trigger: "Use the qa-walkthrough skill.", source: "builtin", folder: "" }),
];

function names() {
  return screen.queryAllByRole("listitem").map((item) => item.getAttribute("aria-label"));
}

describe("filterSkills", () => {
  it("matches the name, the description or the trigger, ignoring case", () => {
    expect(filterSkills(ROWS, "LINT").map((r) => r.name)).toEqual(["lint-fix"]);
    expect(filterSkills(ROWS, "ship").map((r) => r.name)).toEqual(["deploy-site", "qa-walkthrough"]);
    expect(filterSkills(ROWS, "the site").map((r) => r.name)).toEqual(["deploy-site"]);
    expect(filterSkills(ROWS, "  ").map((r) => r.name)).toEqual(["deploy-site", "lint-fix", "qa-walkthrough"]);
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
    expect(within(cabin).getByRole("listitem", { name: "deploy-site" })).toHaveTextContent("deploy the site");
    expect(within(screen.getByRole("list", { name: "Project skills" })).getByRole("listitem")).toHaveTextContent(
      "lint-fix",
    );
    expect(screen.getByRole("list", { name: "Built-in skills" })).toBeInTheDocument();
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
      within(screen.getByRole("listitem", { name: "lint-fix" })).getByRole("button", { name: "Use" }),
    );
    expect(onUse).toHaveBeenLastCalledWith("Use the lint-fix skill.");
  });

  it("Open folder names the skill, never a path, and a built-in has none", async () => {
    api.getSkills.mockResolvedValue({ rows: ROWS });
    api.openSkillFolder.mockRejectedValue("lint-fix isn't in the skills list any more.");
    render(<Skills onUse={() => {}} onOpenConnectors={() => {}} />);

    const lint = await screen.findByRole("listitem", { name: "lint-fix" });
    await userEvent.click(within(lint).getByRole("button", { name: "Open folder" }));
    expect(api.openSkillFolder).toHaveBeenCalledWith("project", "lint-fix");
    expect(await screen.findByRole("status")).toHaveTextContent("lint-fix isn't in the skills list any more.");
    const builtin = screen.getByRole("listitem", { name: "qa-walkthrough" });
    expect(within(builtin).queryByRole("button", { name: "Open folder" })).toBeNull();
  });

  it("the web app lists skills without Open folder", async () => {
    api.getSkills.mockResolvedValue({ rows: ROWS });
    render(<Skills onUse={() => {}} onOpenConnectors={() => {}} remote />);

    await screen.findByRole("listitem", { name: "deploy-site" });
    expect(screen.queryByRole("button", { name: "Open folder" })).toBeNull();
    expect(screen.getAllByRole("button", { name: "Use" })).toHaveLength(3);
  });

  it("says so when there are none, refreshes, and links to Connectors", async () => {
    api.getSkills.mockResolvedValueOnce({ rows: [] }).mockResolvedValueOnce({ rows: [ROWS[1]] });
    const onOpenConnectors = vi.fn();
    render(<Skills onUse={() => {}} onOpenConnectors={onOpenConnectors} />);

    expect(await screen.findByText(SKILLS_EMPTY)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(await screen.findByRole("listitem", { name: "lint-fix" })).toBeInTheDocument();
    expect(api.getSkills).toHaveBeenCalledTimes(2);
    await userEvent.click(screen.getByRole("button", { name: "Connectors" }));
    expect(onOpenConnectors).toHaveBeenCalledTimes(1);
  });
});

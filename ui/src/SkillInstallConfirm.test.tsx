import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "./api";
import { SkillInstallConfirm } from "./SkillInstallConfirm";

vi.mock("./api", async () => {
  const actual = await vi.importActual<typeof import("./api")>("./api");
  return {
    ...actual,
    confirmSkillInstall: vi.fn(),
  };
});

describe("SkillInstallConfirm", () => {
  beforeEach(() => {
    vi.mocked(api.confirmSkillInstall).mockReset();
  });

  it("shows source and destination, then installs on approval", async () => {
    vi.mocked(api.confirmSkillInstall).mockResolvedValue({
      page: { rows: [] },
      note: "Installed Weekly Meal Plan. Chat can use it.",
      preview: {
        identifier: "library/weekly-meal-plan",
        source: "Skills Library",
        name: "Weekly Meal Plan",
        description: "Plan meals",
        destination: "/home/user/.config/GrokHub/skills/weekly-meal-plan",
        kind: "skill",
      },
    });
    const onDone = vi.fn();
    const onCancel = vi.fn();
    render(
      <SkillInstallConfirm
        preview={{
          identifier: "library/weekly-meal-plan",
          source: "Skills Library",
          name: "Weekly Meal Plan",
          description: "Plan meals",
          destination: "/home/user/.config/GrokHub/skills/weekly-meal-plan",
          kind: "skill",
        }}
        onDone={onDone}
        onCancel={onCancel}
      />,
    );
    expect(screen.getByRole("heading", { name: /Install “Weekly Meal Plan”\?/ })).toBeInTheDocument();
    expect(screen.getByText("Skills Library")).toBeInTheDocument();
    expect(
      screen.getByText("/home/user/.config/GrokHub/skills/weekly-meal-plan"),
    ).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Install" }));
    await waitFor(() =>
      expect(api.confirmSkillInstall).toHaveBeenCalledWith("library/weekly-meal-plan"),
    );
    expect(onDone).toHaveBeenCalledWith("Installed Weekly Meal Plan. Chat can use it.");
    expect(onCancel).not.toHaveBeenCalled();
  });

  it("lists bundle skills in the confirm dialog", () => {
    render(
      <SkillInstallConfirm
        preview={{
          identifier: "bundle/backend-dev",
          source: "Skill bundle",
          name: "backend-dev",
          description: "Backend feature work",
          destination: "/home/user/.config/GrokHub/skill-bundles/backend-dev.yaml",
          kind: "bundle",
          skills: ["tdd", "land-it", "follow-through"],
        }}
        onDone={() => {}}
        onCancel={() => {}}
      />,
    );
    expect(
      screen.getByText("Bundle skills: tdd, land-it, follow-through."),
    ).toBeInTheDocument();
  });
});

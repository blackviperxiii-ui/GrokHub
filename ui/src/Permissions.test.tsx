import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { PermissionPage, RuleKind } from "./api";

const api = vi.hoisted(() => ({
  getPermissions: vi.fn<() => Promise<PermissionPage>>(),
  addPermissionRule: vi.fn<(rule: string, kind: RuleKind) => Promise<PermissionPage>>(),
  removePermissionRule: vi.fn<(index: number, rule: string) => Promise<PermissionPage>>(),
  rememberGrant: vi.fn<(command: string) => Promise<PermissionPage>>(),
  forgetGrant: vi.fn<(command: string) => Promise<PermissionPage>>(),
}));

vi.mock("./api", () => api);

import { Permissions, RULES_NOTE } from "./Permissions";

const EMPTY: PermissionPage = { rules: [], imported: [], grants: [] };

describe("Permissions", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
  });

  it("adds a deny rule and removes it by its place in the list", async () => {
    api.getPermissions.mockResolvedValue(EMPTY);
    const one: PermissionPage = { ...EMPTY, rules: [{ action: "deny", rule: "WebFetch(domain:example.com)" }] };
    api.addPermissionRule.mockResolvedValue(one);
    api.removePermissionRule.mockResolvedValue(EMPTY);
    render(<Permissions />);
    expect(await screen.findByText("No saved rules.")).toBeVisible();
    expect(screen.getByText(RULES_NOTE)).toBeVisible();

    await userEvent.click(screen.getByRole("button", { name: "Deny" }));
    expect(screen.getByRole("button", { name: "Deny" })).toHaveAttribute("aria-pressed", "true");
    const box = screen.getByLabelText("Rule");
    await userEvent.type(box, "WebFetch(domain:example.com){Enter}");
    expect(api.addPermissionRule).toHaveBeenCalledWith("WebFetch(domain:example.com)", "deny");
    expect(box).toHaveValue("");
    expect(screen.getByRole("status")).toHaveTextContent("Saved: Deny WebFetch(domain:example.com).");

    const saved = within(screen.getByRole("list", { name: "Saved rules" }));
    await userEvent.click(
      within(saved.getByRole("listitem", { name: "Deny WebFetch(domain:example.com)" })).getByRole("button", {
        name: "Remove",
      }),
    );
    expect(api.removePermissionRule).toHaveBeenCalledWith(0, "WebFetch(domain:example.com)");
    expect(screen.getByText("No saved rules.")).toBeVisible();
  });

  it("keeps the typed rule and shows why when it is refused, and lists project rules read only", async () => {
    api.getPermissions.mockResolvedValue({ ...EMPTY, imported: [{ action: "allow", rule: "Bash(npm test)" }] });
    api.addPermissionRule.mockRejectedValue("That rule could not be read: unknown tool prefix: Nope.");
    render(<Permissions />);
    const project = within(await screen.findByRole("list", { name: "Project rules" }));
    expect(project.getByRole("listitem", { name: "Allow Bash(npm test)" })).toBeVisible();
    expect(project.queryByRole("button")).toBeNull();

    const box = screen.getByLabelText("Rule");
    await userEvent.type(box, "Nope(x){Enter}");
    expect(box).toHaveValue("Nope(x)");
    expect(screen.getByRole("status")).toHaveTextContent(
      "That rule could not be read: unknown tool prefix: Nope.",
    );
  });

  it("remembers a command for this project and forgets it", async () => {
    api.getPermissions.mockResolvedValue(EMPTY);
    api.rememberGrant.mockResolvedValue({ ...EMPTY, grants: ["cargo build"] });
    api.forgetGrant.mockResolvedValue(EMPTY);
    render(<Permissions />);
    expect(await screen.findByText("No remembered commands for this project.")).toBeVisible();
    await userEvent.type(screen.getByLabelText("Command"), "cargo build{Enter}");
    expect(api.rememberGrant).toHaveBeenCalledWith("cargo build");
    expect(screen.getByRole("status")).toHaveTextContent("Remembered cargo build for this project.");
    const grants = within(screen.getByRole("list", { name: "Remembered commands" }));
    await userEvent.click(grants.getByRole("button", { name: "Remove" }));
    expect(api.forgetGrant).toHaveBeenCalledWith("cargo build");
    expect(screen.getByText("No remembered commands for this project.")).toBeVisible();
  });
});

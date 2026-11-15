import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { GithubRan, GithubTool, GithubView } from "./api";

const api = vi.hoisted(() => ({
  getGithub: vi.fn<() => Promise<GithubView>>(),
  saveGithubToken: vi.fn<(token: string) => Promise<GithubView>>(),
  runGithub: vi.fn<(tool: GithubTool) => Promise<GithubRan>>(),
}));

vi.mock("./api", () => api);

import { Github, TOKEN_SET, TOKEN_UNSET } from "./Github";

const FAKE = "sk-abcdefghijklmnopqrstuv";
const SAVED: GithubView = { tokenSet: true, status: "GitHub token saved." };

describe("Github", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
  });

  it("saves a pasted token, clears the field, and only then offers the calls", async () => {
    api.getGithub.mockResolvedValue({ tokenSet: false, status: "" });
    api.saveGithubToken.mockResolvedValue(SAVED);
    render(<Github />);

    expect(await screen.findByText(TOKEN_UNSET)).toBeVisible();
    expect(screen.getByRole("button", { name: "Who am I" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "List repos" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Save token" })).toBeDisabled();

    const field = screen.getByLabelText("GitHub token");
    expect(field).toHaveAttribute("type", "password");
    await userEvent.type(field, FAKE);
    await userEvent.click(screen.getByRole("button", { name: "Save token" }));

    expect(api.saveGithubToken).toHaveBeenCalledWith(FAKE);
    expect(field).toHaveValue("");
    expect(await screen.findByRole("status")).toHaveTextContent("GitHub token saved.");
    expect(screen.getByText(TOKEN_SET)).toBeVisible();
    expect(screen.getByRole("button", { name: "Who am I" })).toBeEnabled();
  });

  it("shows who the token signs in as", async () => {
    api.getGithub.mockResolvedValue({ ...SAVED, status: "" });
    api.runGithub.mockResolvedValue({ summary: "Signed in as octo (Robin Lee) · 3 public repos", repos: [] });
    render(<Github />);
    await userEvent.click(await screen.findByRole("button", { name: "Who am I" }));
    expect(api.runGithub).toHaveBeenCalledWith("user");
    expect(await screen.findByRole("status")).toHaveTextContent("Signed in as octo (Robin Lee) · 3 public repos");
    expect(screen.queryByRole("list", { name: "GitHub repositories" })).toBeNull();
  });

  it("lists repositories with whether each is private", async () => {
    api.getGithub.mockResolvedValue({ ...SAVED, status: "" });
    api.runGithub.mockResolvedValue({
      summary: "2 repositories, most recently updated first.",
      repos: [
        { name: "octo/my-app", private: false, description: "An app" },
        { name: "octo/notes", private: true, description: "" },
      ],
    });
    render(<Github />);
    await userEvent.click(await screen.findByRole("button", { name: "List repos" }));
    expect(api.runGithub).toHaveBeenCalledWith("listRepos");
    const list = within(await screen.findByRole("list", { name: "GitHub repositories" }));
    expect(list.getByRole("listitem", { name: "octo/my-app" })).toHaveTextContent("octo/my-appPublicAn app");
    expect(list.getByRole("listitem", { name: "octo/notes" })).toHaveTextContent("octo/notesPrivate");
  });

  it("shows GitHub's refusal and drops the last answer", async () => {
    api.getGithub.mockResolvedValue({ ...SAVED, status: "" });
    api.runGithub
      .mockResolvedValueOnce({
        summary: "1 repository.",
        repos: [{ name: "octo/my-app", private: false, description: "" }],
      })
      .mockRejectedValueOnce("GitHub didn't accept this token (401 Bad credentials). Paste a new one.");
    render(<Github />);
    await userEvent.click(await screen.findByRole("button", { name: "List repos" }));
    expect(await screen.findByRole("list", { name: "GitHub repositories" })).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "List repos" }));
    expect(await screen.findByRole("status")).toHaveTextContent(
      "GitHub didn't accept this token (401 Bad credentials). Paste a new one.",
    );
    expect(screen.queryByRole("list", { name: "GitHub repositories" })).toBeNull();
  });

  it("removes the token only after the question", async () => {
    api.getGithub.mockResolvedValue({ ...SAVED, status: "" });
    api.saveGithubToken.mockResolvedValue({ tokenSet: false, status: "GitHub token removed." });
    render(<Github />);
    await userEvent.click(await screen.findByRole("button", { name: "Remove token…" }));
    expect(api.saveGithubToken).not.toHaveBeenCalled();
    const ask = screen.getByRole("group", { name: "Remove the GitHub token?" });
    await userEvent.click(within(ask).getByRole("button", { name: "Remove" }));
    expect(api.saveGithubToken).toHaveBeenCalledWith("");
    expect(await screen.findByRole("status")).toHaveTextContent("GitHub token removed.");
    expect(screen.getByRole("button", { name: "Who am I" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Remove token…" })).toBeNull();
  });
});

import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { HookRow, HooksPage } from "./api";

const api = vi.hoisted(() => ({
  getHooks: vi.fn<() => Promise<HooksPage>>(),
  setHooksTrust: vi.fn<(trusted: boolean) => Promise<HooksPage>>(),
}));

vi.mock("./api", () => api);

import { HOOKS_EMPTY, Hooks, TRUSTED_NOTE, UNTRUSTED_NOTE, hookStatus } from "./Hooks";

const WAITS_FOR_TRUST = "Runs after you trust this folder";

const MINE: HookRow = {
  event: "PreToolUse",
  matcher: "",
  command: "echo user",
  kind: "command",
  origin: "user",
  path: "/home/user/.grok/hooks/u.json",
  runs: true,
  note: "",
};

const PROJECT: HookRow = {
  event: "PreToolUse",
  matcher: "Bash",
  command: "echo project",
  kind: "command",
  origin: "project",
  path: "/home/user/my-app/.grok/hooks/p.json",
  runs: false,
  note: WAITS_FOR_TRUST,
};

const UNTRUSTED: HooksPage = {
  folder: "/home/user/my-app",
  trusted: false,
  projectHooks: 1,
  rows: [MINE, PROJECT],
  status: "",
};

const TRUSTED: HooksPage = {
  ...UNTRUSTED,
  trusted: true,
  rows: [MINE, { ...PROJECT, runs: true, note: "" }],
  status: "Trusted /home/user/my-app. Its hooks run from your next message.",
};

describe("Hooks", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
  });

  it("marks a project's hooks as waiting, and trusts the folder only after the question", async () => {
    api.getHooks.mockResolvedValue(UNTRUSTED);
    api.setHooksTrust.mockResolvedValue(TRUSTED);
    render(<Hooks />);

    const list = within(await screen.findByRole("list", { name: "Hooks list" }));
    expect(list.getByRole("listitem", { name: "PreToolUse · any tool echo user" })).toHaveTextContent("Yours");
    const project = list.getByRole("listitem", { name: "PreToolUse · Bash echo project" });
    expect(project).toHaveTextContent(`This project · ${WAITS_FOR_TRUST}`);
    expect(project).toHaveTextContent("/home/user/my-app/.grok/hooks/p.json");
    expect(screen.getByText(new RegExp(UNTRUSTED_NOTE))).toBeVisible();

    await userEvent.click(screen.getByRole("button", { name: "Trust this folder's hooks…" }));
    expect(api.setHooksTrust).not.toHaveBeenCalled();
    const ask = screen.getByRole("group", { name: "Trust this folder's hooks?" });
    await userEvent.click(within(ask).getByRole("button", { name: "Trust" }));

    expect(api.setHooksTrust).toHaveBeenCalledWith(true);
    expect(await screen.findByRole("status")).toHaveTextContent(
      "Trusted /home/user/my-app. Its hooks run from your next message.",
    );
    expect(screen.getByRole("listitem", { name: "PreToolUse · Bash echo project" })).not.toHaveTextContent(
      WAITS_FOR_TRUST,
    );
    expect(screen.getByText(new RegExp(TRUSTED_NOTE))).toBeVisible();
    expect(screen.getByRole("button", { name: "Stop trusting" })).toBeEnabled();
  });

  it("Keep leaves the folder untrusted", async () => {
    api.getHooks.mockResolvedValue(UNTRUSTED);
    render(<Hooks />);
    await userEvent.click(await screen.findByRole("button", { name: "Trust this folder's hooks…" }));
    await userEvent.click(screen.getByRole("button", { name: "Keep" }));
    expect(api.setHooksTrust).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Trust this folder's hooks…" })).toBeEnabled();
  });

  it("stops trusting without a question", async () => {
    api.getHooks.mockResolvedValue({ ...TRUSTED, status: "" });
    api.setHooksTrust.mockResolvedValue({
      ...UNTRUSTED,
      status: "Stopped trusting /home/user/my-app. Its hooks don't run from your next message.",
    });
    render(<Hooks />);
    await userEvent.click(await screen.findByRole("button", { name: "Stop trusting" }));
    expect(api.setHooksTrust).toHaveBeenCalledWith(false);
    expect(await screen.findByRole("status")).toHaveTextContent(
      "Stopped trusting /home/user/my-app. Its hooks don't run from your next message.",
    );
    expect(screen.getByRole("listitem", { name: "PreToolUse · Bash echo project" })).toHaveTextContent(
      WAITS_FOR_TRUST,
    );
  });

  it("shows why trust was refused and what the file holds now", async () => {
    const home = { ...UNTRUSTED, folder: "/home/user", projectHooks: 0, rows: [MINE] };
    api.getHooks.mockResolvedValue(home);
    api.setHooksTrust.mockRejectedValue(
      "Your home folder and the filesystem root can't be trusted: that would trust every project below them. Open a project folder first.",
    );
    render(<Hooks />);
    await userEvent.click(await screen.findByRole("button", { name: "Trust this folder's hooks…" }));
    await userEvent.click(screen.getByRole("button", { name: "Trust" }));
    expect(await screen.findByRole("status")).toHaveTextContent(
      "Your home folder and the filesystem root can't be trusted",
    );
    expect(api.getHooks).toHaveBeenCalledTimes(2);
    expect(screen.getByRole("button", { name: "Trust this folder's hooks…" })).toBeEnabled();
  });

  it("shows the shell's reason a hook doesn't run beside whose it is", () => {
    const http: HookRow = {
      ...MINE,
      kind: "http",
      command: "https://hooks.example.com/[redacted]",
      runs: false,
      note: "http hooks don't run here yet",
    };
    expect(hookStatus(http)).toBe("Yours · http hooks don't run here yet");
    expect(hookStatus(PROJECT)).toBe(`This project · ${WAITS_FOR_TRUST}`);
    expect(hookStatus(MINE)).toBe("Yours");
  });

  it("says where hooks come from when there are none", async () => {
    api.getHooks.mockResolvedValue({ ...UNTRUSTED, projectHooks: 0, rows: [] });
    render(<Hooks />);
    expect(await screen.findByText(HOOKS_EMPTY)).toBeVisible();
    expect(screen.queryByRole("list", { name: "Hooks list" })).toBeNull();
  });
});

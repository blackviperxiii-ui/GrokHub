import { describe, expect, it } from "vitest";
import { shellCommandOf, shownText } from "./shellLine";

/** The exact message `grokhub_core::shell_line::prompt("git status")` builds. */
const GIT_STATUS =
  "Run this shell command in the project folder, exactly as written and once, " +
  "then show me what it printed and its exit code. Don't change it or run " +
  "anything else.\n\n```sh\ngit status\n```";

describe("shellLine", () => {
  it("reads the engine's message back as the line typed", () => {
    expect(shellCommandOf(GIT_STATUS)).toBe("git status");
    expect(shownText(GIT_STATUS)).toBe("$ git status");
    const ticks = GIT_STATUS.replace("```sh\ngit status\n```", "````sh\necho ```x```\n````");
    expect(shellCommandOf(ticks)).toBe("echo ```x```");
  });

  it("leaves any other text as it is", () => {
    expect(shellCommandOf("hello")).toBeNull();
    expect(shownText("hello")).toBe("hello");
    expect(shellCommandOf(`${GIT_STATUS}\nmore`)).toBeNull();
    expect(shellCommandOf(GIT_STATUS.replace("git status", "ls\npwd"))).toBeNull();
    expect(shellCommandOf(GIT_STATUS.replace("```sh", "````sh"))).toBeNull();
    expect(shellCommandOf(GIT_STATUS.replace("```sh", "```bash"))).toBeNull();
  });
});

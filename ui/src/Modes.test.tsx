import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { EFFORT_TIP, ElicitCard, ModeChips, type ElicitItem } from "./Modes";

vi.mock("./api", () => ({}));

function chips(over: Partial<Parameters<typeof ModeChips>[0]> = {}) {
  return (
    <ModeChips
      mode="chat"
      onMode={() => {}}
      approvals="ask"
      onApprovals={() => {}}
      busy={false}
      remote={false}
      {...over}
    />
  );
}

function card(over: Partial<ElicitItem> = {}): ElicitItem {
  return {
    kind: "elicit",
    id: "e1",
    server: "Question",
    message: "Which color?",
    mode: "form",
    url: "",
    fieldName: "answer",
    fieldTitle: "Your answer",
    secret: false,
    answer: null,
    ...over,
  };
}

describe("ModeChips", () => {
  it("every chip has its tooltip", () => {
    render(chips());
    const tips = screen.getAllByRole("button").map((b) => [b.textContent, b.title]);
    expect(tips).toEqual([
      ["Chat", "Normal chat. Grok can edit files and use tools in your work folder. Everyday work."],
      [
        "Plan",
        "Grok writes a plan before changing things, and changes nothing until you approve it. Use this for bigger or riskier work.",
      ],
      ["btw", "A look-only side ask. Grok can read and answer, but changes nothing."],
      ["Ask", "It asks before it changes a file or runs a command."],
      ["Auto", "A quick check approves routine steps and asks you about the rest."],
      [
        "Always",
        "Files change and commands run without asking. Deleting, sending, paying and credentials still ask.",
      ],
    ]);
    expect(screen.getByText("Auto effort")).toHaveAttribute("title", EFFORT_TIP);
  });

  it("picks a mode and an approval", async () => {
    const onMode = vi.fn();
    const onApprovals = vi.fn();
    render(chips({ onMode, onApprovals }));
    await userEvent.click(screen.getByRole("button", { name: "Plan" }));
    await userEvent.click(screen.getByRole("button", { name: "Always" }));
    expect(onMode).toHaveBeenCalledWith("plan");
    expect(onApprovals).toHaveBeenCalledWith("always");
  });

  it("waits for Settings before the approvals can change", () => {
    render(chips({ approvals: null }));
    expect(screen.getByRole("button", { name: "Ask" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Chat" })).toBeEnabled();
  });
});

describe("ElicitCard", () => {
  it("a secret field is a password box and its value never shows", async () => {
    const onAnswer = vi.fn();
    render(
      <ElicitCard
        item={card({ server: "github", fieldName: "token", fieldTitle: "Token", secret: true })}
        onAnswer={onAnswer}
      />,
    );
    expect(screen.getByRole("group", { name: "github needs something from you" })).toBeVisible();
    const box = screen.getByLabelText("Token");
    expect(box).toHaveAttribute("type", "password");
    await userEvent.type(box, "sk-abcdefghijklmnopqrstuv{Enter}");
    expect(onAnswer).toHaveBeenCalledWith("accept", "sk-abcdefghijklmnopqrstuv");
  });

  it("a link card finishes or declines", async () => {
    const onAnswer = vi.fn();
    render(
      <ElicitCard
        item={card({ server: "linear", mode: "url", url: "https://example.com/auth", fieldName: null })}
        onAnswer={onAnswer}
      />,
    );
    expect(screen.getByText("https://example.com/auth")).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "I've finished" }));
    await userEvent.click(screen.getByRole("button", { name: "Decline" }));
    expect(onAnswer.mock.calls).toEqual([
      ["accept", ""],
      ["decline", ""],
    ]);
  });

  it("a declined plan says it stays in plan mode", () => {
    render(<ElicitCard item={card({ server: "Plan", fieldName: null, answer: "decline" })} onAnswer={() => {}} />);
    expect(screen.getByText("Kept in plan mode.")).toBeVisible();
    expect(screen.queryByRole("button")).toBeNull();
  });
});

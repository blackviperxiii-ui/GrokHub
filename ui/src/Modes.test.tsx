import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { ElicitCard, type ElicitItem } from "./Modes";

vi.mock("./api", () => ({}));

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
    const open = vi.spyOn(window, "open").mockReturnValue(null);
    const link = screen.getByRole("link", { name: "Open https://example.com/auth" });
    expect(link).toHaveAttribute("href", "https://example.com/auth");
    await userEvent.click(link);
    expect(open).toHaveBeenCalledWith("https://example.com/auth", "_blank", "noopener,noreferrer");
    open.mockRestore();
    await userEvent.click(screen.getByRole("button", { name: "I've finished" }));
    await userEvent.click(screen.getByRole("button", { name: "Decline" }));
    expect(onAnswer.mock.calls).toEqual([
      ["accept", ""],
      ["decline", ""],
    ]);
  });

  it("a question with options is tappable and sends the chosen label", async () => {
    const onAnswer = vi.fn();
    const item = card({
      message: "1. Which fix?\n- Pin the crate (Recommended): smallest change\n- Upgrade everything\n\nReply with the option label.",
      options: [
        { label: "Pin the crate", description: "smallest change", recommended: true },
        { label: "Upgrade everything", description: "", recommended: false },
      ],
    });
    const view = render(<ElicitCard item={item} onAnswer={onAnswer} />);
    expect(screen.getByText("1. Which fix?")).toBeVisible();
    expect(screen.queryByText(/Reply with the option label/)).toBeNull();
    const choices = screen.getAllByRole("button").filter((b) => b.closest(".answer-options"));
    expect(choices.map((b) => b.textContent)).toEqual([
      "Pin the crate Recommendedsmallest change",
      "Upgrade everything",
    ]);
    expect(screen.getAllByText("Recommended")).toHaveLength(1);
    expect(screen.getByPlaceholderText("Or type your own answer")).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: /Upgrade everything/ }));
    expect(onAnswer.mock.calls).toEqual([["accept", "Upgrade everything"]]);
    view.rerender(<ElicitCard item={{ ...item, answer: "accept" }} onAnswer={onAnswer} />);
    expect(screen.getByText("You chose: Upgrade everything")).toBeVisible();
    expect(screen.queryByRole("group", { name: "Options" })).toBeNull();
  });

  it("a question without options shows no option buttons", () => {
    render(<ElicitCard item={card()} onAnswer={() => {}} />);
    expect(screen.queryByRole("group", { name: "Options" })).toBeNull();
    expect(screen.getByText("Which color?")).toBeVisible();
    expect(screen.getByPlaceholderText("Your answer")).toBeVisible();
  });

  it("a declined plan says it stays in plan mode", () => {
    render(<ElicitCard item={card({ server: "Plan", fieldName: null, answer: "decline" })} onAnswer={() => {}} />);
    expect(screen.getByText("Kept in plan mode.")).toBeVisible();
    expect(screen.queryByRole("button")).toBeNull();
  });
});

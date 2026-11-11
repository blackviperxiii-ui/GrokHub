import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { changeRequest, ElicitCard, PLAN_SENT_BACK, type ElicitItem } from "./Modes";

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

  it("an editable plan approves with the text as edited", async () => {
    const onAnswer = vi.fn();
    const item = card({ server: "Plan", fieldName: null, message: "1. Draft" });
    const view = render(<ElicitCard item={item} onAnswer={onAnswer} editable />);
    const plan = screen.getByRole("textbox", { name: "Plan" });
    expect(plan).toHaveValue("1. Draft");
    await userEvent.clear(plan);
    // An emptied plan is not a plan to approve.
    expect(screen.getByRole("button", { name: "Approve plan" })).toBeDisabled();
    await userEvent.type(plan, "1. Edited");
    await userEvent.click(screen.getByRole("button", { name: "Approve plan" }));
    expect(onAnswer.mock.calls).toEqual([["accept", "1. Edited", "plan"]]);
    view.rerender(<ElicitCard item={{ ...item, answer: "accept" }} onAnswer={onAnswer} editable />);
    expect(screen.queryByRole("textbox")).toBeNull();
    expect(screen.getByRole("group", { name: "The plan" })).toHaveTextContent(
      "1. EditedApproved. Loadout can make the changes now.",
    );
  });

  it("ask for changes needs a note and sends it as a reply", async () => {
    const onAnswer = vi.fn();
    const item = card({ server: "Plan", fieldName: null, message: "1. Draft" });
    const view = render(<ElicitCard item={item} onAnswer={onAnswer} editable />);
    const ask = screen.getByRole("button", { name: "Ask for changes" });
    expect(ask).toBeDisabled();
    await userEvent.type(screen.getByRole("textbox", { name: "What should change" }), "make step 2 smaller");
    expect(ask).toBeEnabled();
    await userEvent.click(ask);
    expect(onAnswer.mock.calls).toEqual([["accept", "Change the plan: make step 2 smaller", "reply"]]);
    view.rerender(<ElicitCard item={{ ...item, answer: "accept" }} onAnswer={onAnswer} editable />);
    expect(PLAN_SENT_BACK).toBe("Sent back for changes.");
    expect(screen.getByText("Sent back for changes.")).toBeVisible();
    expect(screen.queryByText("Approved. Loadout can make the changes now.")).toBeNull();
  });

  it("a change request carries the plan only when it was edited", () => {
    expect(changeRequest(" make step 2 smaller ", "1. Draft\n", "1. Draft")).toBe(
      "Change the plan: make step 2 smaller",
    );
    expect(changeRequest("make step 2 smaller", "1. Edited", "1. Draft")).toBe(
      "Change the plan: make step 2 smaller\n\nThe plan as I edited it:\n1. Edited",
    );
  });

  it("a plan card outside the agent window is not editable", async () => {
    const onAnswer = vi.fn();
    render(<ElicitCard item={card({ server: "Plan", fieldName: null, message: "1. Draft" })} onAnswer={onAnswer} />);
    expect(screen.queryByRole("textbox")).toBeNull();
    expect(screen.queryByRole("button", { name: "Ask for changes" })).toBeNull();
    expect(screen.getByText("1. Draft")).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Approve plan" }));
    await userEvent.click(screen.getByRole("button", { name: "Keep planning" }));
    expect(onAnswer.mock.calls).toEqual([
      ["accept", ""],
      ["decline", ""],
    ]);
  });
});

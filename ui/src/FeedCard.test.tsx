import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CardButtons } from "./FeedCard";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

function reduced(matches: boolean) {
  vi.stubGlobal(
    "matchMedia",
    vi.fn(() => ({ matches, addEventListener: () => {}, removeEventListener: () => {} })),
  );
}

function row(liked: boolean, onLike = vi.fn()) {
  return render(
    <CardButtons liked={liked} busy={false} onLike={onLike} onDislike={vi.fn()} onDismiss={vi.fn()}>
      <button type="button">Discuss</button>
    </CardButtons>,
  );
}

describe("CardButtons like → heart", () => {
  it("shows ♡ Like when not liked, and Like is the first button", () => {
    reduced(false);
    row(false);
    const group = screen.getByRole("group", { name: "Card actions" });
    const buttons = within(group).getAllByRole("button");
    expect(buttons[0]).toHaveAccessibleName("Like");
    expect(buttons[0]).toHaveAttribute("aria-pressed", "false");
    expect(buttons[0].textContent).toBe("♡ Like");
  });

  it("a liked card shows only the filled heart and stays pressed", () => {
    reduced(false);
    row(true);
    const like = screen.getByRole("button", { name: "Like" });
    expect(like).toHaveAttribute("aria-pressed", "true");
    expect(like.textContent).toBe("♥");
    expect(like).not.toHaveClass("card-like-pop");
  });

  it("tapping Like adds the pop class; once liked the heart stays without pop; unlike has no pop", async () => {
    reduced(false);
    const onLike = vi.fn();
    const { rerender } = row(false, onLike);
    const like = screen.getByRole("button", { name: "Like" });
    await userEvent.click(like);
    expect(onLike).toHaveBeenCalledTimes(1);
    expect(like).toHaveClass("card-like-pop");
    expect(like.textContent).toBe("♥");

    rerender(
      <CardButtons liked={true} busy={false} onLike={onLike} onDislike={vi.fn()} onDismiss={vi.fn()}>
        <button type="button">Discuss</button>
      </CardButtons>,
    );
    const heart = screen.getByRole("button", { name: "Like" });
    expect(heart).toHaveAttribute("aria-pressed", "true");
    expect(heart.textContent).toBe("♥");
    expect(heart).not.toHaveClass("card-like-pop");
    await userEvent.click(heart);
    expect(onLike).toHaveBeenCalledTimes(2);
    expect(heart).not.toHaveClass("card-like-pop");
  });

  it("reduced motion skips the pop class and swaps state instantly", async () => {
    reduced(true);
    const onLike = vi.fn();
    const { rerender } = row(false, onLike);
    const like = screen.getByRole("button", { name: "Like" });
    await userEvent.click(like);
    expect(onLike).toHaveBeenCalledTimes(1);
    expect(like).not.toHaveClass("card-like-pop");
    rerender(
      <CardButtons liked={true} busy={false} onLike={onLike} onDislike={vi.fn()} onDismiss={vi.fn()}>
        <button type="button">Discuss</button>
      </CardButtons>,
    );
    const heart = screen.getByRole("button", { name: "Like" });
    expect(heart).toHaveAttribute("aria-pressed", "true");
    expect(heart.textContent).toBe("♥");
    expect(heart).not.toHaveClass("card-like-pop");
  });
});

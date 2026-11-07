import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { SIDE_ANSWERING, SIDE_FAILED, SIDE_KEEP, SideCards, withSide, type Side } from "./Side";

vi.mock("./api", () => ({}));

function side(over: Partial<Side> = {}): Side {
  return { id: "c~side-1", question: "what port is it on?", answer: "", live: true, failed: false, ...over };
}

describe("SideCards", () => {
  it("shows nothing until a side question is asked", () => {
    const { container } = render(<SideCards sides={[]} onStop={() => {}} onTell={() => {}} onDismiss={() => {}} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("a running side answer offers Stop only", async () => {
    const onStop = vi.fn();
    render(<SideCards sides={[side()]} onStop={onStop} onTell={() => {}} onDismiss={() => {}} />);
    expect(screen.getByRole("group", { name: "On the side: what port is it on?" })).toBeVisible();
    expect(screen.getByText(SIDE_ANSWERING)).toBeVisible();
    expect(screen.queryByRole("button", { name: "Tell the main task" })).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(onStop).toHaveBeenCalledOnce();
  });

  it("a finished answer can go to the main task or be dismissed", async () => {
    const onTell = vi.fn();
    const onDismiss = vi.fn();
    const done = side({ live: false, answer: "It's on 8080." });
    render(<SideCards sides={[done]} onStop={() => {}} onTell={onTell} onDismiss={onDismiss} />);
    expect(screen.getByText("It's on 8080.")).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Tell the main task" }));
    await userEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(onTell).toHaveBeenCalledWith(done);
    expect(onDismiss).toHaveBeenCalledWith(done);
    expect(screen.queryByRole("button", { name: "Stop" })).toBeNull();
  });

  it("a failed side answer says so and can only be dismissed", () => {
    render(
      <SideCards sides={[side({ live: false, failed: true })]} onStop={() => {}} onTell={() => {}} onDismiss={() => {}} />,
    );
    expect(screen.getByText(SIDE_FAILED)).toBeVisible();
    expect(screen.queryByRole("button", { name: "Tell the main task" })).toBeNull();
  });
});

describe("withSide", () => {
  it("keeps every running answer and the newest finished ones", () => {
    let all: Side[] = [];
    for (let i = 0; i < SIDE_KEEP + 2; i += 1) all = withSide(all, side({ id: `c~side-${i}`, live: false }));
    all = withSide(all, side({ id: "c~side-live" }));
    expect(all.filter((s) => !s.live)).toHaveLength(SIDE_KEEP);
    expect(all.at(-1)?.id).toBe("c~side-live");
    expect(all[0].id).toBe("c~side-2");
  });
});

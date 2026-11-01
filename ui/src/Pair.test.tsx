import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const transport = vi.hoisted(() => ({
  pair: vi.fn<(code: string) => Promise<void>>(),
  inShell: vi.fn(() => false),
  paired: vi.fn(() => false),
  onPairChange: vi.fn(() => () => {}),
}));

vi.mock("./transport", () => transport);
vi.mock("./App", () => ({ App: () => <p>The app</p> }));

import { Pair, PAIR_HELP, Root, takeCodeFromLink } from "./Pair";

describe("Pair", () => {
  beforeEach(() => {
    for (const fn of Object.values(transport)) fn.mockClear();
    transport.pair.mockReset();
    transport.inShell.mockReturnValue(false);
    window.history.replaceState(null, "", "/");
  });

  it("takes the code off a pairing link and leaves the address clean", () => {
    window.history.replaceState(null, "", "/chat?x=1#code=pair%2Dabc&v=1");
    expect(takeCodeFromLink()).toBe("pair-abc");
    expect(window.location.hash).toBe("");
    expect(window.location.pathname + window.location.search).toBe("/chat?x=1");
    expect(takeCodeFromLink()).toBe("");
  });

  it("pairs from a pairing link without typing", async () => {
    transport.pair.mockResolvedValue();
    const onPaired = vi.fn();
    window.history.replaceState(null, "", "/#code=pair-xyz");
    render(<Pair onPaired={onPaired} />);
    await waitFor(() => expect(onPaired).toHaveBeenCalledTimes(1));
    expect(transport.pair).toHaveBeenCalledWith("pair-xyz");
  });

  it("a pairing link shows it is connecting until the pairing is done", async () => {
    let done: () => void = () => {};
    transport.pair.mockImplementation(() => new Promise<void>((resolve) => (done = resolve)));
    const onPaired = vi.fn();
    window.history.replaceState(null, "", "/#code=pair-xyz");
    render(<Pair onPaired={onPaired} />);
    expect(screen.getByRole("button", { name: "Connecting…" })).toBeDisabled();
    expect(transport.pair).toHaveBeenCalledWith("pair-xyz");
    done();
    expect(await screen.findByRole("button", { name: "Connect" })).toBeDisabled();
    expect(onPaired).toHaveBeenCalledTimes(1);
  });

  it("shows why a typed code didn't pair", async () => {
    transport.pair.mockRejectedValue(new Error("GrokHub on your computer refused that pairing code."));
    const onPaired = vi.fn();
    render(<Pair onPaired={onPaired} />);
    expect(screen.getByText(PAIR_HELP)).toBeInTheDocument();
    const code = screen.getByLabelText("Pairing code");
    expect(code).toHaveAttribute("type", "password");
    await userEvent.type(code, "pair-bad");
    await userEvent.click(screen.getByRole("button", { name: "Connect" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "GrokHub on your computer refused that pairing code.",
    );
    expect(transport.pair).toHaveBeenCalledWith("pair-bad");
    expect(onPaired).not.toHaveBeenCalled();
  });

  it("opens the app straight away in the shell, and the pairing screen in a browser", () => {
    transport.inShell.mockReturnValue(true);
    const { unmount } = render(<Root />);
    expect(screen.getByText("The app")).toBeInTheDocument();
    unmount();

    transport.inShell.mockReturnValue(false);
    render(<Root />);
    expect(screen.getByLabelText("Pairing code")).toBeInTheDocument();
    expect(screen.queryByText("The app")).toBeNull();
  });
});

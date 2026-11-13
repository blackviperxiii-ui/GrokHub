import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import { act } from "react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  GrokSignInPoll,
  GrokSignInStart,
  ImagineKind,
  ImagineMade,
  ImaginePage,
  ImagineTile,
} from "./api";

const api = vi.hoisted(() => ({
  getImagine: vi.fn<() => Promise<ImaginePage>>(),
  imagineGenerate:
    vi.fn<(prompt: string, aspect: string, count: number, kind: ImagineKind) => Promise<ImagineMade>>(),
  imagineCancel: vi.fn<() => Promise<boolean>>(),
  imagineImage: vi.fn<(name: string) => Promise<string>>(),
  pulseImage: vi.fn<(name: string) => Promise<string>>(),
  imagineSignInStart: vi.fn<() => Promise<GrokSignInStart>>(),
  imagineSignInPoll: vi.fn<() => Promise<GrokSignInPoll>>(),
  imagineSignOut: vi.fn<() => Promise<ImaginePage>>(),
}));

vi.mock("./api", () => api);
vi.mock("./external", () => ({ openExternal: () => Promise.resolve() }));

import { GALLERY_EMPTY, Imagine, makingLine, NEED_SIGNIN } from "./Imagine";
import { requestImagine } from "./imagineAsk";

function tile(over: Partial<ImagineTile> = {}): ImagineTile {
  return {
    id: "gen-1",
    name: "gen-1_a.png",
    title: "snowy cabin",
    prompt: "a snowy cabin",
    createdMs: 1,
    video: false,
    tall: false,
    ...over,
  };
}

const EMPTY: ImaginePage = { tiles: [], ready: true, account: null, canSignIn: false };

describe("Imagine", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
    api.imagineImage.mockImplementation((name) => Promise.resolve(`data:image/png;base64,${name}`));
    api.imagineCancel.mockResolvedValue(true);
  });

  it("starts square, one image, video off", async () => {
    api.getImagine.mockResolvedValue(EMPTY);
    render(<Imagine />);
    expect(await screen.findByText(GALLERY_EMPTY)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Square" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "1" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Image" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Video" })).toHaveAttribute("aria-pressed", "false");
    expect(screen.getByRole("button", { name: "Make" })).toBeDisabled();
  });

  it("generate shows progress, then the new tiles", async () => {
    api.getImagine.mockResolvedValue(EMPTY);
    let finish: (made: ImagineMade) => void = () => {};
    api.imagineGenerate.mockReturnValue(new Promise((resolve) => (finish = resolve)));
    render(<Imagine />);
    await screen.findByText(GALLERY_EMPTY);
    await userEvent.type(screen.getByLabelText("What to make"), "a snowy cabin");
    await userEvent.click(screen.getByRole("button", { name: "Wide" }));
    await userEvent.click(screen.getByRole("button", { name: "2" }));
    await userEvent.click(screen.getByRole("button", { name: "Make" }));

    expect(api.imagineGenerate).toHaveBeenCalledWith("a snowy cabin", "3:2", 2, "image");
    expect(screen.getByRole("status")).toHaveTextContent("Making 2 images…");
    expect(screen.getByRole("button", { name: "Stop" })).toBeInTheDocument();

    finish({
      page: { tiles: [tile({ id: "gen-2", name: "gen-2_a.png" }), tile()], ready: true, account: null, canSignIn: false },
      made: 2,
      note: "",
    });
    const gallery = await screen.findByRole("list", { name: "Gallery" });
    expect(within(gallery).getAllByRole("listitem")).toHaveLength(2);
    expect(screen.getByRole("status")).toHaveTextContent("Made 2.");
    expect(screen.getByRole("button", { name: "Make" })).toBeEnabled();
    await waitFor(() => expect(api.imagineImage).toHaveBeenCalledWith("gen-2_a.png"));
  });

  it("a video is one, with no count chips", async () => {
    api.getImagine.mockResolvedValue(EMPTY);
    api.imagineGenerate.mockReturnValue(new Promise(() => {}));
    render(<Imagine />);
    await screen.findByText(GALLERY_EMPTY);
    await userEvent.click(screen.getByRole("button", { name: "Video" }));
    expect(screen.queryByRole("group", { name: "How many" })).toBeNull();
    await userEvent.type(screen.getByLabelText("What to make"), "waves");
    await userEvent.click(screen.getByRole("button", { name: "Make" }));
    expect(api.imagineGenerate).toHaveBeenCalledWith("waves", "1:1", 1, "video");
    expect(screen.getByRole("status")).toHaveTextContent(makingLine("video", 1));
  });

  it("Stop calls cancel and the run ends as stopped", async () => {
    api.getImagine.mockResolvedValue(EMPTY);
    let fail: (err: string) => void = () => {};
    api.imagineGenerate.mockReturnValue(new Promise((_, reject) => (fail = reject)));
    render(<Imagine />);
    await screen.findByText(GALLERY_EMPTY);
    await userEvent.type(screen.getByLabelText("What to make"), "waves");
    await userEvent.click(screen.getByRole("button", { name: "Make" }));
    await userEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(api.imagineCancel).toHaveBeenCalledTimes(1);
    fail("Stopped.");
    expect(await screen.findByRole("alert")).toHaveTextContent("Stopped.");
    expect(screen.getByRole("button", { name: "Make" })).toBeEnabled();
  });

  it("the lightbox opens a tile with Download and closes", async () => {
    api.getImagine.mockResolvedValue({ tiles: [tile()], ready: true, account: null, canSignIn: false });
    render(<Imagine />);
    await userEvent.click(await screen.findByRole("button", { name: "Open snowy cabin" }));
    const box = await screen.findByRole("dialog", { name: "Image" });
    expect(within(box).getByRole("link", { name: "Download" })).toHaveAttribute("download", "gen-1_a.png");
    expect(within(box).getByRole("link", { name: "Download" })).toHaveAttribute(
      "href",
      "data:image/png;base64,gen-1_a.png",
    );
    await userEvent.click(within(box).getByRole("button", { name: "Close" }));
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("a video tile opens in a player", async () => {
    api.getImagine.mockResolvedValue({
      tiles: [tile({ id: "gen-3", name: "gen-3_a.mp4", title: "waves", video: true })],
      ready: true,
      account: null,
      canSignIn: false,
    });
    api.imagineImage.mockResolvedValue("data:video/mp4;base64,AAAA");
    render(<Imagine />);
    await userEvent.click(await screen.findByRole("button", { name: "Open waves" }));
    const box = await screen.findByRole("dialog", { name: "Image" });
    expect(box.querySelector("video")).toHaveAttribute("src", "data:video/mp4;base64,AAAA");
    expect(api.imagineImage).toHaveBeenCalledTimes(1);
  });

  it("with no sign-in it asks for one on the page, and a code signs Imagine in", async () => {
    api.getImagine
      .mockResolvedValueOnce({ tiles: [], ready: false, account: null, canSignIn: true })
      .mockResolvedValueOnce({
        tiles: [],
        ready: true,
        account: { name: null, email: "sam@example.com" },
        canSignIn: true,
      });
    api.imagineSignInStart.mockResolvedValue({
      userCode: "ABCD-EFGH",
      verificationUri: "https://accounts.x.ai/device",
      verificationUriComplete: null,
      interval: 0.01,
      expiresIn: 600,
    });
    // The poll answers only once the code has been seen: on a slow runner a
    // poll that is ready at once replaces the code before the test looks.
    let approve: (poll: { status: "ready"; signedIn: { name: string; email: string } }) => void = () => {};
    api.imagineSignInPoll.mockReturnValue(new Promise((ok) => (approve = ok)));
    render(<Imagine />);
    const box = await screen.findByRole("region", { name: "Imagine sign-in" });
    await userEvent.click(within(box).getByRole("button", { name: "Sign in with Grok" }));
    expect(api.imagineSignInStart).toHaveBeenCalledTimes(1);
    expect(await within(box).findByLabelText("Sign-in code")).toHaveTextContent("ABCD-EFGH");
    approve({ status: "ready", signedIn: { name: "Sam", email: "sam@example.com" } });
    expect(await within(box).findByText("Signed in as sam@example.com.")).toBeInTheDocument();
    expect(api.getImagine).toHaveBeenCalledTimes(2);
    await userEvent.type(screen.getByLabelText("What to make"), "a cabin");
    expect(screen.getByRole("button", { name: "Make" })).toBeEnabled();
  });

  it("Imagine's own sign-in can be signed out here; a chat sign-in alone shows nothing", async () => {
    api.getImagine.mockResolvedValue({
      tiles: [],
      ready: true,
      account: { name: null, email: "sam@example.com" },
      canSignIn: true,
    });
    api.imagineSignOut.mockResolvedValue({ tiles: [], ready: true, account: null, canSignIn: true });
    render(<Imagine />);
    const box = await screen.findByRole("region", { name: "Imagine sign-in" });
    await userEvent.click(within(box).getByRole("button", { name: "Sign out" }));
    expect(api.imagineSignOut).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(screen.queryByRole("region", { name: "Imagine sign-in" })).toBeNull());
  });

  it("with no key it asks to sign in and Make waits", async () => {
    api.getImagine.mockResolvedValue({ tiles: [], ready: false, account: null, canSignIn: false });
    render(<Imagine />);
    expect(await screen.findByRole("alert")).toHaveTextContent(NEED_SIGNIN);
    await userEvent.type(screen.getByLabelText("What to make"), "a cabin");
    expect(screen.getByRole("button", { name: "Make" })).toBeDisabled();
  });

  it("a refused key shows the same pointer", async () => {
    api.getImagine.mockResolvedValue(EMPTY);
    api.imagineGenerate.mockRejectedValue(NEED_SIGNIN);
    render(<Imagine />);
    await screen.findByText(GALLERY_EMPTY);
    await userEvent.type(screen.getByLabelText("What to make"), "a cabin");
    await userEvent.click(screen.getByRole("button", { name: "Make" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Sign in to Imagine first. Sign in with Grok here, or add an xAI API key in Settings → Account.",
    );
  });

  it("+ New project clears the prompt, the result line and the error, keeps the gallery and focuses the prompt", async () => {
    api.getImagine.mockResolvedValue({ tiles: [tile()], ready: true, account: null, canSignIn: false });
    api.imagineGenerate.mockResolvedValue({ page: { tiles: [tile()], ready: true, account: null, canSignIn: false }, made: 1, note: "" });
    render(<Imagine />);
    await screen.findByRole("list", { name: "Gallery" });
    await userEvent.type(screen.getByLabelText("What to make"), "a red barn");
    await userEvent.click(screen.getByRole("button", { name: "Make" }));
    expect(await screen.findByText("Made 1.")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "+ New project" }));
    expect(screen.getByLabelText("What to make")).toHaveValue("");
    expect(screen.getByLabelText("What to make")).toHaveFocus();
    expect(screen.queryByText("Made 1.")).toBeNull();
    expect(screen.getByRole("list", { name: "Gallery" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Make" })).toBeDisabled();
  });

  it("+ New project clears an error", async () => {
    api.getImagine.mockResolvedValue(EMPTY);
    api.imagineGenerate.mockRejectedValue("The image service is busy. Try again.");
    render(<Imagine />);
    await screen.findByText(GALLERY_EMPTY);
    await userEvent.type(screen.getByLabelText("What to make"), "a red barn");
    await userEvent.click(screen.getByRole("button", { name: "Make" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("The image service is busy. Try again.");
    await userEvent.click(screen.getByRole("button", { name: "+ New project" }));
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("a slash prompt makes one video at square", async () => {
    api.getImagine.mockResolvedValue(EMPTY);
    api.imagineGenerate.mockResolvedValue({
      page: { tiles: [tile({ video: true })], ready: true, account: null, canSignIn: false },
      made: 1,
      note: "",
    });
    render(<Imagine />);
    await screen.findByText(GALLERY_EMPTY);
    act(() => {
      requestImagine("video", "waves");
    });
    await waitFor(() => expect(api.imagineGenerate).toHaveBeenCalledWith("waves", "1:1", 1, "video"));
    expect(await screen.findByText("Made 1.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Video" })).toHaveAttribute("aria-pressed", "true");
  });
});

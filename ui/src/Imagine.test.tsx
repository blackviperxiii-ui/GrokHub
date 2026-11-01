import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ImagineKind, ImagineMade, ImaginePage, ImagineTile } from "./api";

const api = vi.hoisted(() => ({
  getImagine: vi.fn<() => Promise<ImaginePage>>(),
  imagineGenerate:
    vi.fn<(prompt: string, aspect: string, count: number, kind: ImagineKind) => Promise<ImagineMade>>(),
  imagineCancel: vi.fn<() => Promise<boolean>>(),
  imagineImage: vi.fn<(name: string) => Promise<string>>(),
  pulseImage: vi.fn<(name: string) => Promise<string>>(),
}));

vi.mock("./api", () => api);

import { GALLERY_EMPTY, Imagine, makingLine, NEED_SIGNIN } from "./Imagine";

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

const EMPTY: ImaginePage = { tiles: [], ready: true };

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
      page: { tiles: [tile({ id: "gen-2", name: "gen-2_a.png" }), tile()], ready: true },
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
    api.getImagine.mockResolvedValue({ tiles: [tile()], ready: true });
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
    });
    api.imagineImage.mockResolvedValue("data:video/mp4;base64,AAAA");
    render(<Imagine />);
    await userEvent.click(await screen.findByRole("button", { name: "Open waves" }));
    const box = await screen.findByRole("dialog", { name: "Image" });
    expect(box.querySelector("video")).toHaveAttribute("src", "data:video/mp4;base64,AAAA");
    expect(api.imagineImage).toHaveBeenCalledTimes(1);
  });

  it("with no key it points to the cabin's sign-in and Make waits", async () => {
    api.getImagine.mockResolvedValue({ tiles: [], ready: false });
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
      "Sign in to Imagine in the cabin. Or add an xAI API key in Settings → Account on your computer.",
    );
  });
});

import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import { act } from "react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  BattlenetPage,
  ChatEvent,
  ImagineKind,
  ImagineMade,
  ImaginePage,
  ImagineTile,
  LibraryPage,
  MissionPage,
  Settings,
  SlashEffect,
  SlashHit,
  WallNews,
} from "./api";

const api = vi.hoisted(() => ({
  getSettings: vi.fn<() => Promise<Settings>>(),
  sendChat: vi.fn<(id: string, text: string) => Promise<string>>(),
  slashHits: vi.fn<(draft: string) => Promise<SlashHit[]>>(),
  runSlash: vi.fn<(line: string) => Promise<SlashEffect>>(),
  onChatEvent: vi.fn<(handler: (ev: { conversationId: string; event: ChatEvent; helper?: boolean }) => void) => Promise<() => void>>(),
  chatPicture: vi.fn<(path: string) => Promise<string>>(),
  chatMedia: vi.fn<(path: string) => Promise<string>>(),
  saveChatMediaToLibrary: vi.fn<(path: string, prompt: string) => Promise<ImagineTile>>(),
  getImagine: vi.fn<() => Promise<ImaginePage>>(),
  imagineGenerate:
    vi.fn<(prompt: string, aspect: string, count: number, kind: ImagineKind) => Promise<ImagineMade>>(),
  imagineCancel: vi.fn<() => Promise<boolean>>(),
  imagineImage: vi.fn<(name: string) => Promise<string>>(),
  onWallNews: vi.fn<(handler: (news: WallNews) => void) => Promise<() => void>>(),
  getLibrary: vi.fn<() => Promise<LibraryPage>>(),
  getBattlenet: vi.fn<() => Promise<BattlenetPage>>(),
  getMission: vi.fn<() => Promise<MissionPage>>(),
  libraryCover: vi.fn<() => Promise<string | null>>(),
}));

vi.mock("./api", async () => {
  const { chatTabApiStubs } = await import("./chatTabApiStubs");
  return {
    ...chatTabApiStubs(),
    quickChips: () => Promise.resolve([]),
    chipOutcome: () => Promise.resolve(true),
    getSettings: api.getSettings,
    sendChat: api.sendChat,
    slashHits: api.slashHits,
    runSlash: api.runSlash,
    onChatEvent: api.onChatEvent,
    chatPicture: api.chatPicture,
    chatMedia: api.chatMedia,
    saveChatMediaToLibrary: api.saveChatMediaToLibrary,
    getImagine: api.getImagine,
    imagineGenerate: api.imagineGenerate,
    imagineCancel: api.imagineCancel,
    imagineImage: api.imagineImage,
    onWallNews: api.onWallNews,
    getLibrary: api.getLibrary,
    getBattlenet: api.getBattlenet,
    getMission: api.getMission,
    libraryCover: api.libraryCover,
    libraryPlay: vi.fn(),
    battlenetPlay: vi.fn(),
    battlenetOpenFolder: vi.fn(),
    closeChat: vi.fn(),
    addAutomation: vi.fn(),
    downloadModels: vi.fn(),
    answerElicit: vi.fn(),
    saveSettings: vi.fn(),
    answerPermission: vi.fn(),
    cancelChat: vi.fn(),
    getStartChat: () => Promise.resolve(null),
    loadChat: vi.fn(),
    helperQuestions: () => Promise.resolve([]),
    onEventGap: () => Promise.resolve(() => {}),
    onFileDrop: () => Promise.resolve(() => {}),
    stageAttachment: vi.fn(),
    stageAttachmentPaths: vi.fn(),
    pickAttachments: vi.fn(),
    removeAttachment: vi.fn(),
  };
});

import { Chat, IMAGINE_PROMPT_HINT, makingLine } from "./Chat";
import { CREATIONS_EMPTY, Library, paintingLine, WALL_RETRY, wallLine } from "./Library";

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

const SETTINGS: Settings = {
  closeToTray: true,
  hasApiKey: true,
  workspace: "",
  workspacePath: "/home/me/GrokHub-Work",
  approvals: "ask",
  desktopControl: false,
  theme: "system",
  version: "3.1.0",
  summonHotkey: false,
  notifications: true,
  dockPanels: false,
  dockLayout: { panes: [{ tabs: ["chat"], active: 0, share: 1000, id: "pane-1" }], focused: 0 },
};

const EMPTY_PAGE: ImaginePage = { tiles: [], ready: true, account: null, canSignIn: false };
const EMPTY_XP = {
  level: 1,
  title: "Rookie",
  xp: 0,
  nextLevelXp: 100,
  frac: 0,
  today: 0,
  week: {
    days: [false, false, false, false, false, false, false],
    today: 0,
    active: 0,
    target: 3,
    inARow: 0,
    played: 0,
    freezes: 0,
  },
};

describe("Chat media generation and Library Creations", () => {
  let pushEvent: (ev: { conversationId: string; event: ChatEvent; helper?: boolean }) => void;

  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
    pushEvent = () => {};
    api.getSettings.mockResolvedValue(SETTINGS);
    api.slashHits.mockResolvedValue([]);
    api.onChatEvent.mockImplementation((handler) => {
      pushEvent = handler;
      return Promise.resolve(() => {});
    });
    api.imagineImage.mockImplementation((name) => Promise.resolve(`data:image/png;base64,${name}`));
    api.imagineCancel.mockResolvedValue(true);
    api.onWallNews.mockResolvedValue(() => {});
    api.getLibrary.mockResolvedValue({ steamFound: false, games: [] });
    api.getBattlenet.mockResolvedValue({ found: false, games: [] });
    api.getMission.mockResolvedValue({ mission: null, xp: EMPTY_XP });
    api.libraryCover.mockResolvedValue(null);
  });

  function wallNews(unlisten: () => void = () => {}) {
    let tell: (news: WallNews) => void = () => {};
    api.onWallNews.mockImplementation((handler) => {
      tell = handler;
      return Promise.resolve(unlisten);
    });
    return (news: WallNews) => act(() => tell(news));
  }

  it("/imagine shows progress with Stop, then renders the generated image inline with Lightbox and Download", async () => {
    let finish: (made: ImagineMade) => void = () => {};
    api.runSlash.mockResolvedValueOnce({ kind: "imagine", media: "image", prompt: "a snowy cabin" });
    api.imagineGenerate.mockReturnValueOnce(new Promise((resolve) => (finish = resolve)));

    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "/imagine a snowy cabin{Enter}");

    expect(api.imagineGenerate).toHaveBeenCalledWith("a snowy cabin", "1:1", 1, "image");
    expect(await screen.findByText("Making 1 image…")).toBeInTheDocument();
    expect(makingLine("image", 1)).toBe("Making 1 image…");
    expect(makingLine("image", 2)).toBe("Making 2 images…");
    expect(screen.getByRole("button", { name: "Stop" })).toBeInTheDocument();

    act(() =>
      finish({
        page: { tiles: [tile()], ready: true, account: null, canSignIn: false },
        made: 1,
        note: "",
      }),
    );

    expect(await screen.findByText("Made 1.")).toBeInTheDocument();
    const img = await screen.findByRole("img", { name: "snowy cabin" });
    expect(img).toHaveAttribute("src", "data:image/png;base64,gen-1_a.png");
    expect(screen.getByText("Saved to Library")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Open" }));
    const box = await screen.findByRole("dialog", { name: "Image" });
    expect(within(box).getByRole("link", { name: "Download" })).toHaveAttribute("download", "gen-1_a.png");
    await userEvent.click(within(box).getByRole("button", { name: "Close" }));
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("/imagine-video generates a video inline, and Stop cancels an in-flight run", async () => {
    let fail: (err: string) => void = () => {};
    api.runSlash.mockResolvedValueOnce({ kind: "imagine", media: "video", prompt: "waves" });
    api.imagineGenerate.mockReturnValueOnce(new Promise((_, reject) => (fail = reject)));

    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "/imagine-video waves{Enter}");

    expect(api.imagineGenerate).toHaveBeenCalledWith("waves", "1:1", 1, "video");
    expect(await screen.findByText("Making a video… this can take a few minutes.")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(api.imagineCancel).toHaveBeenCalledTimes(1);

    act(() => fail("Stopped."));
    expect(await screen.findByRole("alert")).toHaveTextContent("Stopped.");
  });

  it("bare /imagine prompts the user and fills the composer", async () => {
    api.runSlash.mockResolvedValueOnce({ kind: "imagine", media: "image", prompt: "" });
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "/imagine{Enter}");

    expect(await screen.findByText(IMAGINE_PROMPT_HINT)).toBeInTheDocument();
    expect(screen.getByLabelText("Message")).toHaveValue("/imagine ");
    expect(api.imagineGenerate).not.toHaveBeenCalled();
  });

  it("tool-generated image and video render inline in Chat and save to Library", async () => {
    let resolveTurn: () => void = () => {};
    let chatId = "";
    api.sendChat.mockImplementation((id) => {
      chatId = id;
      return new Promise((resolve) => {
        resolveTurn = () => resolve(id);
      });
    });
    const imgPath = "/home/me/.config/app/sessions/c1/media/image-1.png";
    const vidPath = "/home/me/.config/app/sessions/c1/media/video-2.mp4";
    api.chatPicture.mockResolvedValue("data:image/png;base64,IMGDATA");
    api.chatMedia.mockResolvedValue("data:video/mp4;base64,VIDDATA");
    api.saveChatMediaToLibrary
      .mockResolvedValueOnce(tile({ id: "lib-img", name: "lib-img_a.png", title: "image_generate" }))
      .mockResolvedValueOnce(tile({ id: "lib-vid", name: "lib-vid_a.mp4", title: "video_generate", video: true }));

    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "make a picture and a short video{Enter}");
    await waitFor(() => expect(chatId).not.toBe(""));

    act(() => {
      pushEvent({
        conversationId: chatId,
        event: {
          kind: "tool",
          id: "t-img",
          title: "image_generate",
          status: "completed",
          detail: `IMAGINE: ${imgPath}`,
          picture: imgPath,
        },
      });
      pushEvent({
        conversationId: chatId,
        event: {
          kind: "tool",
          id: "t-vid",
          title: "video_generate",
          status: "completed",
          detail: `IMAGINE: ${vidPath}`,
          video: vidPath,
        },
      });
    });

    const img = await screen.findByRole("img", { name: "The picture this made" });
    expect(img).toHaveAttribute("src", "data:image/png;base64,IMGDATA");
    const vid = await screen.findByLabelText("The video this made");
    expect(vid.tagName).toBe("VIDEO");
    expect(vid).toHaveAttribute("src", "data:video/mp4;base64,VIDDATA");

    const saveButtons = screen.getAllByRole("button", { name: "Save to Library" });
    expect(saveButtons).toHaveLength(2);
    await userEvent.click(saveButtons[0]);
    expect(api.saveChatMediaToLibrary).toHaveBeenCalledWith(imgPath, "image_generate");
    await userEvent.click(saveButtons[1]);
    expect(api.saveChatMediaToLibrary).toHaveBeenCalledWith(vidPath, "video_generate");
    await waitFor(() => expect(screen.getAllByRole("button", { name: "Saved to Library" })).toHaveLength(2));

    act(() => {
      pushEvent({ conversationId: chatId, event: { kind: "done", stop: "end_turn" } });
      resolveTurn();
    });
  });

  it("Library Creations shows saved Imagine images and videos with Lightbox, Download, and living wall status", async () => {
    const unlisten = vi.fn();
    const tell = wallNews(unlisten);
    api.getImagine
      .mockResolvedValueOnce(EMPTY_PAGE)
      .mockResolvedValue({
        ...EMPTY_PAGE,
        tiles: [
          tile({ id: "abc", name: "abc_a.png", title: "Snow porch" }),
          tile({ id: "vid-1", name: "vid-1_a.mp4", title: "Ocean waves", video: true }),
        ],
      });
    api.imagineImage.mockImplementation((name) =>
      Promise.resolve(name.endsWith(".mp4") ? "data:video/mp4;base64,AAAA" : `data:image/png;base64,${name}`),
    );

    const { unmount } = render(<Library />);
    await userEvent.click(screen.getByRole("button", { name: "Creations" }));
    expect(await screen.findByText(CREATIONS_EMPTY)).toBeVisible();

    tell({ state: "painting", title: "Snow porch" });
    expect(screen.getByRole("status")).toHaveTextContent("Painting a wall cover: Snow porch");
    expect(paintingLine("Snow porch")).toBe("Painting a wall cover: Snow porch");
    await userEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(api.imagineCancel).toHaveBeenCalledTimes(1);

    tell({ state: "painted", title: "Snow porch" });
    expect(await screen.findByText("Snow porch")).toBeVisible();
    expect(screen.getByRole("status")).toHaveTextContent("New cover on the wall: Snow porch");
    expect(wallLine("Snow porch")).toBe("New cover on the wall: Snow porch");

    // Open saved video in Lightbox player.
    await userEvent.click(screen.getByRole("button", { name: "Open Ocean waves" }));
    const box = await screen.findByRole("dialog", { name: "Image" });
    expect(box.querySelector("video")).toHaveAttribute("src", "data:video/mp4;base64,AAAA");
    expect(within(box).getByRole("link", { name: "Download" })).toHaveAttribute("download", "vid-1_a.mp4");
    await userEvent.click(within(box).getByRole("button", { name: "Close" }));

    tell({ state: "failed" });
    expect(screen.getByRole("status")).toHaveTextContent(WALL_RETRY);
    expect(WALL_RETRY).toBe("The wall cover didn't come. It tries again in 15 minutes.");

    unmount();
    await waitFor(() => expect(unlisten).toHaveBeenCalledTimes(1));
  });
});

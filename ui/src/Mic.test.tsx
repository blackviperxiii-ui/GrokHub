import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Settings } from "./api";

const api = vi.hoisted(() => ({
  transcribe: vi.fn<(wav: string) => Promise<string>>(),
  sendChat: vi.fn<(...args: unknown[]) => Promise<string>>(),
}));

vi.mock("./api", () => ({
  quickChips: () => Promise.resolve([]),
  chipOutcome: () => Promise.resolve(true),
  // No chat saved from an earlier launch.
  getLastChat: () => Promise.resolve(null),
  loadChat: vi.fn(),
  transcribe: api.transcribe,
  sendChat: api.sendChat,
  getSettings: () =>
    Promise.resolve({
      closeToTray: true,
      hasApiKey: true,
      workspace: "",
      workspacePath: "/home/user",
      approvals: "ask",
      desktopControl: false,
      theme: "system",
      version: "3.5.0",
      summonHotkey: false,
      notifications: true,
    } satisfies Settings),
  saveSettings: vi.fn(),
  answerElicit: vi.fn(),
  answerPermission: vi.fn(),
  cancelChat: vi.fn(),
  closeChat: vi.fn(() => Promise.resolve()),
  onChatEvent: () => Promise.resolve(() => {}),
  helperQuestions: () => Promise.resolve([]),
  onEventGap: () => Promise.resolve(() => {}),
  // Attachments have their own tests (AttachChips.test.tsx).
  onFileDrop: () => Promise.resolve(() => {}),
  stageAttachment: vi.fn(),
  stageAttachmentPaths: vi.fn(),
  pickAttachments: vi.fn(),
  removeAttachment: vi.fn(),
}));

import { Chat } from "./Chat";
import { appendDraft, closeMic, MIC_DENIED } from "./Mic";

/** A fake mic at 48 kHz: `feed` plays samples into the open recording. */
function fakeMic() {
  const mic = {
    processor: null as { onaudioprocess: ((e: unknown) => void) | null } | null,
    stopped: 0,
    closed: 0,
    feed(samples: Float32Array) {
      act(() => mic.processor?.onaudioprocess?.({ inputBuffer: { getChannelData: () => samples } }));
    },
  };
  const stream = { getTracks: () => [{ stop: () => (mic.stopped += 1) }] };
  const getUserMedia = vi.fn((_: MediaStreamConstraints) => Promise.resolve(stream));
  class FakeContext {
    sampleRate = 48000;
    destination = {};
    createMediaStreamSource() {
      return { connect: () => {}, disconnect: () => {} };
    }
    createScriptProcessor() {
      const node = { onaudioprocess: null, connect: () => {}, disconnect: () => {} };
      mic.processor = node;
      return node;
    }
    close() {
      mic.closed += 1;
      return Promise.resolve();
    }
  }
  vi.stubGlobal("AudioContext", FakeContext);
  Object.defineProperty(navigator, "mediaDevices", {
    configurable: true,
    value: { getUserMedia },
  });
  return Object.assign(mic, { getUserMedia });
}

function decode(base64: string): DataView {
  const raw = atob(base64);
  const bytes = new Uint8Array(raw.length);
  for (let i = 0; i < raw.length; i++) bytes[i] = raw.charCodeAt(i);
  return new DataView(bytes.buffer);
}

const MIC = /Hold to talk/;

describe("push to talk", () => {
  beforeEach(() => {
    api.transcribe.mockReset();
    api.sendChat.mockReset();
  });
  afterEach(async () => {
    await closeMic();
    vi.unstubAllGlobals();
  });

  it("holding records, letting go transcribes into the draft and sends nothing", async () => {
    const mic = fakeMic();
    api.transcribe.mockResolvedValue("open the build log");
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "please");

    const button = screen.getByRole("button", { name: MIC });
    fireEvent.pointerDown(button);
    await waitFor(() => expect(mic.processor).not.toBeNull());
    expect(screen.getByRole("button", { name: /Listening/ })).toHaveAttribute("aria-pressed", "true");
    // While held, a listening orb takes the mic glyph's place; the button still names it.
    expect(button.querySelector("canvas.orb")).toHaveAttribute("aria-hidden", "true");
    expect(button.querySelector("svg")).toBeNull();
    // 4800 samples at 48 kHz is 0.1 s: 1600 at 16 kHz.
    mic.feed(new Float32Array(4800).fill(0.25));
    fireEvent.pointerUp(button);

    await waitFor(() => expect(api.transcribe).toHaveBeenCalledTimes(1));
    const wav = decode(api.transcribe.mock.calls[0][0]);
    expect(wav.getUint32(24, true)).toBe(16000);
    expect(wav.getUint32(40, true)).toBe(3200);
    expect(wav.getInt16(44, true)).toBe(8191);
    await waitFor(() => expect(screen.getByLabelText("Message")).toHaveValue("please open the build log"));
    expect(api.sendChat).not.toHaveBeenCalled();
    // The mic stays open for the next hold, and closes with the chat.
    expect(mic.stopped).toBe(0);
    await closeMic();
    expect(mic.stopped).toBe(1);
    expect(mic.closed).toBe(1);
  });

  it("asks for echo cancellation, noise suppression and level control", async () => {
    const mic = fakeMic();
    api.transcribe.mockResolvedValue("hi");
    render(<Chat onOpenSettings={() => {}} />);
    const button = screen.getByRole("button", { name: MIC });
    fireEvent.pointerDown(button);
    await waitFor(() => expect(mic.processor).not.toBeNull());
    expect(mic.getUserMedia.mock.calls[0][0]).toEqual({
      audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true, autoGainControl: true },
    });
    mic.feed(new Float32Array(480).fill(0.1));
    fireEvent.pointerUp(button);
    await waitFor(() => expect(api.transcribe).toHaveBeenCalledTimes(1));
  });

  it("the next hold starts at once and keeps the moment before the press", async () => {
    const mic = fakeMic();
    api.transcribe.mockResolvedValue("one");
    render(<Chat onOpenSettings={() => {}} />);
    const button = screen.getByRole("button", { name: MIC });
    fireEvent.pointerDown(button);
    await waitFor(() => expect(mic.processor).not.toBeNull());
    mic.feed(new Float32Array(4800).fill(0.25));
    fireEvent.pointerUp(button);
    await waitFor(() => expect(api.transcribe).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(button).not.toBeDisabled());

    // Talking just before the second press: the first word starts early.
    mic.feed(new Float32Array(4800).fill(0.5));
    fireEvent.pointerDown(button);
    await waitFor(() => expect(button).toHaveAttribute("aria-pressed", "true"));
    mic.feed(new Float32Array(4800).fill(0.25));
    fireEvent.pointerUp(button);
    await waitFor(() => expect(api.transcribe).toHaveBeenCalledTimes(2));
    expect(mic.getUserMedia).toHaveBeenCalledTimes(1);
    const wav = decode(api.transcribe.mock.calls[1][0]);
    expect(wav.getUint32(40, true)).toBe(6400);
    expect(wav.getInt16(44, true)).toBe(16383);
  });

  it("the last word, said as you let go, is kept", async () => {
    const mic = fakeMic();
    api.transcribe.mockResolvedValue("done");
    render(<Chat onOpenSettings={() => {}} />);
    const button = screen.getByRole("button", { name: MIC });
    fireEvent.pointerDown(button);
    await waitFor(() => expect(mic.processor).not.toBeNull());
    mic.feed(new Float32Array(4800).fill(0.25));
    fireEvent.pointerUp(button);
    // Still arriving just after the release.
    mic.feed(new Float32Array(4800).fill(0.25));
    await waitFor(() => expect(api.transcribe).toHaveBeenCalledTimes(1));
    expect(decode(api.transcribe.mock.calls[0][0]).getUint32(40, true)).toBe(6400);
  });

  it("Ctrl+G holds to talk too", async () => {
    const mic = fakeMic();
    api.transcribe.mockResolvedValue("hello");
    render(<Chat onOpenSettings={() => {}} />);
    fireEvent.keyDown(window, { key: "g", ctrlKey: true });
    await waitFor(() => expect(mic.processor).not.toBeNull());
    mic.feed(new Float32Array(480).fill(0.1));
    fireEvent.keyUp(window, { key: "g", ctrlKey: true });
    await waitFor(() => expect(screen.getByLabelText("Message")).toHaveValue("hello"));
  });

  it("a Ctrl+G hold ends when the window loses focus, with no key let go", async () => {
    const mic = fakeMic();
    api.transcribe.mockResolvedValue("said before switching away");
    render(<Chat onOpenSettings={() => {}} />);
    fireEvent.keyDown(window, { key: "g", ctrlKey: true });
    await waitFor(() => expect(mic.processor).not.toBeNull());
    expect(screen.getByRole("button", { name: /Listening/ })).toHaveAttribute("aria-pressed", "true");
    mic.feed(new Float32Array(4800).fill(0.25));
    fireEvent.blur(window);
    await waitFor(() => expect(api.transcribe).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.getByLabelText("Message")).toHaveValue("said before switching away"));
    expect(screen.getByRole("button", { name: MIC })).toHaveAttribute("aria-pressed", "false");
    expect(api.sendChat).not.toHaveBeenCalled();
  });

  it("a denied mic shows a note and sends nothing to transcribe", async () => {
    Object.defineProperty(navigator, "mediaDevices", {
      configurable: true,
      value: {
        getUserMedia: vi.fn(() => Promise.reject(new DOMException("denied", "NotAllowedError"))),
      },
    });
    vi.stubGlobal("AudioContext", class {});
    render(<Chat onOpenSettings={() => {}} />);
    const button = screen.getByRole("button", { name: MIC });
    fireEvent.pointerDown(button);
    fireEvent.pointerUp(button);
    expect(await screen.findByRole("alert")).toHaveTextContent(MIC_DENIED);
    expect(api.transcribe).not.toHaveBeenCalled();
  });

  it("the desktop's refusal shows in its words", async () => {
    const mic = fakeMic();
    api.transcribe.mockRejectedValue("Voice needs an xAI API key. Add one in Settings → Account.");
    render(<Chat onOpenSettings={() => {}} />);
    const button = screen.getByRole("button", { name: MIC });
    fireEvent.pointerDown(button);
    await waitFor(() => expect(mic.processor).not.toBeNull());
    mic.feed(new Float32Array(480));
    fireEvent.pointerUp(button);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Voice needs an xAI API key. Add one in Settings → Account.",
    );
  });

  it("the web app has no mic", () => {
    render(<Chat onOpenSettings={() => {}} remote />);
    expect(screen.queryByRole("button", { name: MIC })).toBeNull();
  });

  it("words go after the typed text with one space", () => {
    expect(appendDraft("", " hi ")).toBe("hi");
    expect(appendDraft("fix this  ", "now")).toBe("fix this now");
    expect(appendDraft("keep", "  ")).toBe("keep");
  });
});

import "@testing-library/jest-dom/vitest";
import { act, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { NyxCard } from "./NyxCard";

interface ObserverEntry {
  target: Element;
  isIntersecting: boolean;
}

let observerCb: ((entries: ObserverEntry[]) => void) | null = null;
let observedEl: Element | null = null;
const disconnectSpy = vi.fn();

class MockIntersectionObserver {
  constructor(cb: (entries: ObserverEntry[]) => void) {
    observerCb = cb;
  }
  observe(el: Element) {
    observedEl = el;
  }
  disconnect() {
    disconnectSpy();
  }
  unobserve() {}
  takeRecords() {
    return [];
  }
}

function triggerIntersect(isIntersecting: boolean) {
  if (!observerCb || !observedEl) throw new Error("IntersectionObserver not observing");
  act(() => {
    observerCb!([{ target: observedEl!, isIntersecting }]);
  });
}

describe("NyxCard", () => {
  let reduced = false;
  let motionListeners: Array<() => void> = [];
  let online = true;
  let visibility: DocumentVisibilityState = "visible";
  let isPaused = true;
  const playSpy = vi.fn(() => {
    isPaused = false;
    return Promise.resolve();
  });
  const pauseSpy = vi.fn(() => {
    isPaused = true;
  });

  beforeEach(() => {
    reduced = false;
    motionListeners = [];
    online = true;
    visibility = "visible";
    isPaused = true;
    observerCb = null;
    observedEl = null;
    disconnectSpy.mockReset();
    playSpy.mockClear();
    pauseSpy.mockClear();
    delete document.documentElement.dataset.reduceMotion;

    Object.defineProperty(globalThis, "IntersectionObserver", {
      writable: true,
      configurable: true,
      value: MockIntersectionObserver,
    });
    Object.defineProperty(window, "matchMedia", {
      writable: true,
      configurable: true,
      value: (query: string) => ({
        matches: query.includes("prefers-reduced-motion") ? reduced : false,
        media: query,
        addEventListener: (_: string, fn: () => void) => motionListeners.push(fn),
        removeEventListener: (_: string, fn: () => void) => {
          motionListeners = motionListeners.filter((l) => l !== fn);
        },
        addListener: () => {},
        removeListener: () => {},
        dispatchEvent: () => false,
        onchange: null,
      }),
    });
    Object.defineProperty(navigator, "onLine", {
      configurable: true,
      get: () => online,
    });
    Object.defineProperty(document, "visibilityState", {
      configurable: true,
      get: () => visibility,
    });
    Object.defineProperty(HTMLMediaElement.prototype, "paused", {
      configurable: true,
      get: () => isPaused,
    });
    Object.defineProperty(HTMLMediaElement.prototype, "play", {
      configurable: true,
      value: playSpy,
    });
    Object.defineProperty(HTMLMediaElement.prototype, "pause", {
      configurable: true,
      value: pauseSpy,
    });
  });

  afterEach(() => {
    delete document.documentElement.dataset.reduceMotion;
  });

  it("renders a video with JPG poster, webm/mp4 sources, and lazy preload", () => {
    const { container } = render(
      <NyxCard
        className="nyx-test"
        url="/assets/nyx-hanging-out.jpg"
        path="brand/nyx-hanging-out.jpg"
        alt="Nyx: Hanging out"
      />,
    );
    const video = container.querySelector("video");
    expect(video).not.toBeNull();
    expect(video).toHaveAttribute("poster", "/assets/nyx-hanging-out.jpg");
    expect(video).toHaveAttribute("preload", "none");
    expect(video).toHaveAttribute("loop");
    expect(video).toHaveAttribute("playsinline");
    expect(video?.muted).toBe(true);
    expect(video).toHaveAttribute("aria-label", "Nyx: Hanging out");

    const sources = Array.from(video!.querySelectorAll("source"));
    expect(sources).toHaveLength(2);
    expect(sources[0]).toHaveAttribute(
      "src",
      "https://github.com/blackviperxiii-ui/GrokHub/releases/download/assets-nyx-anim-1/nyx-hanging-out.webm",
    );
    expect(sources[0]).toHaveAttribute("type", "video/webm");
    expect(sources[1]).toHaveAttribute(
      "src",
      "https://github.com/blackviperxiii-ui/GrokHub/releases/download/assets-nyx-anim-1/nyx-hanging-out.mp4",
    );
    expect(sources[1]).toHaveAttribute("type", "video/mp4");
  });

  it("plays only while visible via IntersectionObserver and pauses off-screen or hidden", () => {
    const { container } = render(
      <NyxCard url="/assets/nyx-working.jpg" path="brand/nyx-working.jpg" />,
    );
    const video = container.querySelector("video")!;
    expect(playSpy).not.toHaveBeenCalled();

    triggerIntersect(true);
    expect(playSpy).toHaveBeenCalledTimes(1);
    expect(video).toHaveAttribute("preload", "metadata");

    triggerIntersect(false);
    expect(pauseSpy).toHaveBeenCalled();

    triggerIntersect(true);
    expect(playSpy).toHaveBeenCalledTimes(2);

    visibility = "hidden";
    act(() => {
      document.dispatchEvent(new Event("visibilitychange"));
    });
    expect(pauseSpy).toHaveBeenCalledTimes(2);

    visibility = "visible";
    act(() => {
      document.dispatchEvent(new Event("visibilitychange"));
    });
    expect(playSpy).toHaveBeenCalledTimes(3);
  });

  it("shows the still JPG under OS prefers-reduced-motion or data-reduce-motion", async () => {
    reduced = true;
    const { container } = render(
      <NyxCard className="home-nyx" url="/assets/nyx.jpg" path="brand/nyx-hanging-out.jpg" />,
    );
    expect(container.querySelector("video")).toBeNull();
    expect(container.querySelector("img")).toHaveAttribute("src", "/assets/nyx.jpg");

    reduced = false;
    act(() => {
      for (const fn of motionListeners) fn();
    });
    expect(container.querySelector("video")).not.toBeNull();

    document.documentElement.dataset.reduceMotion = "true";
    await waitFor(() => expect(container.querySelector("video")).toBeNull());
    expect(container.querySelector("img")).toHaveAttribute("src", "/assets/nyx.jpg");
  });

  it("falls back to the still JPG when offline or on media error", () => {
    online = false;
    const { container } = render(
      <NyxCard url="/assets/nyx.jpg" path="brand/nyx-hanging-out.jpg" />,
    );
    expect(container.querySelector("video")).toBeNull();
    expect(container.querySelector("img")).toHaveAttribute("src", "/assets/nyx.jpg");

    online = true;
    act(() => {
      window.dispatchEvent(new Event("online"));
    });
    const video = container.querySelector("video");
    expect(video).not.toBeNull();

    const lastSource = video!.querySelectorAll("source")[1];
    fireEvent.error(lastSource);
    expect(container.querySelector("video")).toBeNull();
    expect(container.querySelector("img")).toHaveAttribute("src", "/assets/nyx.jpg");
  });

  it("falls back to img when no repo clip path is provided", () => {
    const { container } = render(<NyxCard url="/nyx.jpg" />);
    expect(container.querySelector("video")).toBeNull();
    expect(container.querySelector("img")).toHaveAttribute("src", "/nyx.jpg");
  });
});

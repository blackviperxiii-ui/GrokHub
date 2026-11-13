import { memo, useEffect, useRef, useState, useSyncExternalStore } from "react";
import { clipUrls } from "./nyx";

const REDUCED_MOTION_QUERY = "(prefers-reduced-motion: reduce)";

function readReducedMotion(): boolean {
  if (typeof document !== "undefined" && document.documentElement.dataset.reduceMotion === "true") {
    return true;
  }
  if (typeof window !== "undefined" && typeof window.matchMedia === "function") {
    return window.matchMedia(REDUCED_MOTION_QUERY).matches;
  }
  return false;
}

function subscribeReducedMotion(onChange: () => void): () => void {
  const mql =
    typeof window !== "undefined" && typeof window.matchMedia === "function"
      ? window.matchMedia(REDUCED_MOTION_QUERY)
      : null;
  mql?.addEventListener?.("change", onChange);
  let observer: MutationObserver | null = null;
  if (typeof document !== "undefined" && typeof MutationObserver !== "undefined") {
    observer = new MutationObserver(onChange);
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["data-reduce-motion"],
    });
  }
  return () => {
    mql?.removeEventListener?.("change", onChange);
    observer?.disconnect();
  };
}

function readOnline(): boolean {
  return typeof navigator === "undefined" ? true : navigator.onLine !== false;
}

function subscribeOnline(onChange: () => void): () => void {
  if (typeof window === "undefined") return () => {};
  window.addEventListener("online", onChange);
  window.addEventListener("offline", onChange);
  return () => {
    window.removeEventListener("online", onChange);
    window.removeEventListener("offline", onChange);
  };
}

function tryPlay(el: HTMLVideoElement) {
  el.muted = true;
  try {
    const p = el.play?.();
    if (p && typeof p.catch === "function") p.catch(() => {});
  } catch {
    // Ignore playback rejections; the poster JPG stays visible.
  }
}

function tryPause(el: HTMLVideoElement) {
  if (el.paused) return;
  try {
    el.pause?.();
  } catch {
    // Ignore in environments without full HTMLMediaElement support.
  }
}

export interface NyxCardProps {
  /** Bundled still JPG URL (shown immediately as poster and fallback). */
  url: string;
  /** Repo path (e.g. `brand/nyx-hanging-out.jpg`) used to stream the clip. */
  path?: string | null;
  className?: string;
  alt?: string;
  draggable?: boolean;
  onClick?: () => void;
}

/** Streams a Nyx card's silent WebM/MP4 loop when visible and motion is allowed;
 * shows the bundled JPG as poster first and falls back to it on reduced motion,
 * offline state, or playback error. */
export const NyxCard = memo(function NyxCard({
  url,
  path,
  className,
  alt = "",
  draggable = false,
  onClick,
}: NyxCardProps) {
  const reducedMotion = useSyncExternalStore(subscribeReducedMotion, readReducedMotion, () => false);
  const online = useSyncExternalStore(subscribeOnline, readOnline, () => true);
  const clip = clipUrls(path);
  const [failedUrl, setFailedUrl] = useState<string | null>(null);
  const [seenUrl, setSeenUrl] = useState<string | null>(null);
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const intersectingRef = useRef(false);

  const canAnimate = !reducedMotion && online && clip !== null && failedUrl !== url;

  useEffect(() => {
    if (!canAnimate) return;
    const onOnline = () => setFailedUrl(null);
    window.addEventListener("online", onOnline);
    return () => window.removeEventListener("online", onOnline);
  }, [canAnimate]);

  useEffect(() => {
    const el = videoRef.current;
    if (!canAnimate || !el) return;

    const sync = () => {
      const visible = intersectingRef.current && document.visibilityState !== "hidden";
      if (visible) {
        setSeenUrl(url);
        tryPlay(el);
      } else {
        tryPause(el);
      }
    };

    let observer: IntersectionObserver | null = null;
    if (typeof IntersectionObserver !== "undefined") {
      observer = new IntersectionObserver((entries) => {
        intersectingRef.current = entries.some((entry) => entry.isIntersecting);
        sync();
      });
      observer.observe(el);
    }

    document.addEventListener("visibilitychange", sync);
    return () => {
      intersectingRef.current = false;
      observer?.disconnect();
      document.removeEventListener("visibilitychange", sync);
      tryPause(el);
    };
  }, [canAnimate, url]);

  if (!canAnimate) {
    return (
      <img
        className={className}
        src={url}
        alt={alt}
        aria-hidden={alt ? undefined : true}
        draggable={draggable}
        onClick={onClick}
      />
    );
  }

  return (
    <video
      key={url}
      ref={videoRef}
      className={className}
      poster={url}
      muted
      loop
      playsInline
      preload={seenUrl === url ? "metadata" : "none"}
      aria-label={alt || undefined}
      aria-hidden={alt ? undefined : true}
      draggable={draggable}
      onClick={onClick}
      onError={() => setFailedUrl(url)}
    >
      <source src={clip.webm} type="video/webm" />
      <source src={clip.mp4} type="video/mp4" onError={() => setFailedUrl(url)} />
    </video>
  );
});

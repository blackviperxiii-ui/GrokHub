import { useEffect, useRef, useState, type ReactNode } from "react";
import { pulseImage, type PulseIdea, type PulsePost } from "./api";
import { VerdictLine } from "./PulseFix";
import { ConfirmRow, ExternalLink, useDismiss } from "./ui";

/** Topic emoji with a bundled 3D picture (Fluent Emoji 3D, MIT, see
 * public/emoji3d/LICENSE), by code point without variation selectors. */
export const EMOJI_3D = new Set(
  (
    "1f30e 1f327 1f331 1f389 1f393 1f3a8 1f3ac 1f3ae 1f3b5 1f3c6 1f3e0 1f3e5 1f40d 1f41b 1f427 " +
    "1f48a 1f4a1 1f4b0 1f4bb 1f4bc 1f4c5 1f4c8 1f4ca 1f4da 1f4e3 1f4e6 1f4f0 1f4f1 1f4f7 1f50b " +
    "1f50c 1f511 1f512 1f514 1f525 1f527 1f52c 1f579 1f5d3 1f680 1f697 1f6d2 1f6e0 1f6e1 1f6f0 " +
    "1f914 1f916 1f980 1f9e0 1f9e9 1f9ea 1f9ec 2600 2615 2699 26a0 26a1 26bd 2708 2728 2764"
  ).split(" "),
);

/** A post with no emoji of its own. */
export const DEFAULT_ICON = "📰";

/** The bundled picture for an emoji, or null to show the emoji itself. */
export function emojiImage(icon: string): string | null {
  const code = [...icon]
    .map((c) => c.codePointAt(0) ?? 0)
    .filter((cp) => cp !== 0xfe0f)
    .map((cp) => cp.toString(16))
    .join("-");
  return EMOJI_3D.has(code) ? `${import.meta.env.BASE_URL}emoji3d/${code}.png` : null;
}

/** An idea with no emoji of its own. */
export const IDEA_ICON = "💡";

function TopicIcon({ icon, fallback = DEFAULT_ICON, className = "topic-icon" }: {
  icon: string;
  fallback?: string;
  className?: string;
}) {
  const emoji = icon.trim() || fallback;
  const src = emojiImage(emoji);
  const [broken, setBroken] = useState(false);
  if (src && !broken) {
    return <img className={className} src={src} alt="" aria-hidden="true" onError={() => setBroken(true)} />;
  }
  return (
    <span className={`${className} plain`} aria-hidden="true">
      {emoji}
    </span>
  );
}

/** The writer's snippet: plain text with `[label](url)` cites and `**bold**`.
 * Links open in the browser; nothing else is read as markup. */
export function Snippet({ text }: { text: string }) {
  const parts: ReactNode[] = [];
  const re = /\[([^\]]+)\]\((https?:\/\/[^)\s]+)\)|\*\*([^*]+)\*\*/g;
  let at = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) {
    if (m.index > at) parts.push(text.slice(at, m.index));
    if (m[1] !== undefined) {
      parts.push(
        <ExternalLink key={m.index} href={m[2]}>
          {m[1]}
        </ExternalLink>,
      );
    } else {
      parts.push(<strong key={m.index}>{m[3]}</strong>);
    }
    at = m.index + m[0].length;
  }
  if (at < text.length) parts.push(text.slice(at));
  return <p className="snippet">{parts}</p>;
}

/** Cached images as data URLs, loaded once per name. */
function useImages(names: string[]): string[] {
  const [urls, setUrls] = useState<string[]>([]);
  // `names` is a new array on every page load; its JSON only changes when a name does.
  const key = JSON.stringify(names);
  useEffect(() => {
    let live = true;
    const list: string[] = JSON.parse(key);
    Promise.all(list.map((n) => pulseImage(n).catch(() => ""))).then((got) => {
      if (live) setUrls(got.filter(Boolean));
    });
    return () => {
      live = false;
    };
  }, [key]);
  return urls;
}

/** One picture full size, with Download; `video` plays a clip instead. */
export function Lightbox({
  src,
  name,
  video = false,
  onClose,
}: {
  src: string;
  name: string;
  video?: boolean;
  onClose: () => void;
}) {
  const close = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    close.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  return (
    <div className="lightbox" role="dialog" aria-modal="true" aria-label="Image" onClick={onClose}>
      {video ? (
        <video src={src} controls autoPlay loop onClick={(e) => e.stopPropagation()} />
      ) : (
        <img src={src} alt="" onClick={(e) => e.stopPropagation()} />
      )}
      <div className="lightbox-buttons" onClick={(e) => e.stopPropagation()}>
        <a className="button quiet" href={src} download={name}>
          Download
        </a>
        <button ref={close} type="button" className="quiet" onClick={onClose}>
          Close
        </button>
      </div>
    </div>
  );
}

function Media({ post }: { post: PulsePost }) {
  const urls = useImages(post.images);
  const [open, setOpen] = useState<number | null>(null);
  if (urls.length === 0) return null;
  const one = urls.length === 1;
  return (
    <>
      <div className={one ? "media hero" : "media gallery"}>
        {urls.slice(0, 3).map((src, i) => (
          <button
            key={i}
            type="button"
            className="media-item"
            aria-label={`Open image ${i + 1}`}
            onClick={() => setOpen(i)}
          >
            <img src={src} alt="" />
          </button>
        ))}
      </div>
      {open !== null && urls[open] && (
        <Lightbox src={urls[open]} name={post.images[open] ?? "image"} onClose={() => setOpen(null)} />
      )}
    </>
  );
}

/** "Oct 26, 2026, 3:04 PM" for the menu. */
export function fullDate(ms: number): string {
  return new Date(ms).toLocaleString(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  });
}

export interface FeedCardProps {
  post: PulsePost;
  busy: boolean;
  onLove: () => void;
  onDiscuss: () => void;
  onDelete: () => void;
  onWhy: () => Promise<string>;
  /** Hide it until tomorrow 9 AM. */
  onSnooze?: () => void;
  /** Take it off the page. */
  onDismiss?: () => void;
  /** Take it off and steer the writer away from posts like it. */
  onNotThis?: () => void;
}

/** One Feed post: icon, title, verdict, cited snippet, media; its two buttons
 * below, Discuss (the main one) and Love;
 * time and a menu (date, why, snooze, dismiss, not this, delete) top right.
 * Delete asks first. */
export function FeedCard({
  post,
  busy,
  onLove,
  onDiscuss,
  onDelete,
  onWhy,
  onSnooze,
  onDismiss,
  onNotThis,
}: FeedCardProps) {
  const [menu, setMenu] = useState(false);
  const [why, setWhy] = useState<string | null>(null);
  const [asking, setAsking] = useState(false);
  const corner = useDismiss<HTMLDivElement>(menu, () => setMenu(false));
  const pick = (f: () => void) => () => {
    setMenu(false);
    f();
  };

  async function askWhy() {
    setMenu(false);
    setWhy(await onWhy().catch((err) => String(err)));
  }

  return (
    <li className="card post feed-card" aria-label={post.title}>
      <div className="feed-head">
        <TopicIcon icon={post.icon} />
        <div className="feed-title">{post.title}</div>
        <div className="feed-corner" ref={corner}>
          <span className="card-status">{post.ago}</span>
          <button
            type="button"
            className="quiet icon-button"
            aria-label="More"
            aria-haspopup="menu"
            aria-expanded={menu}
            onClick={() => setMenu((m) => !m)}
          >
            ⋯
          </button>
          {menu && (
            <div className="feed-menu" role="menu">
              <div className="feed-menu-date" role="none">
                {fullDate(post.createdAt)}
              </div>
              <button type="button" role="menuitem" onClick={() => void askWhy()}>
                Why I created this
              </button>
              {onSnooze && (
                <button type="button" role="menuitem" disabled={busy} onClick={pick(onSnooze)}>
                  Snooze until 9 AM
                </button>
              )}
              {onDismiss && (
                <button type="button" role="menuitem" disabled={busy} onClick={pick(onDismiss)}>
                  Dismiss
                </button>
              )}
              {onNotThis && (
                <button type="button" role="menuitem" disabled={busy} onClick={pick(onNotThis)}>
                  Not this
                </button>
              )}
              <button
                type="button"
                role="menuitem"
                className="danger"
                disabled={busy}
                onClick={pick(() => setAsking(true))}
              >
                Delete…
              </button>
            </div>
          )}
        </div>
      </div>
      <VerdictLine verdict={post.verdict} />
      {post.summary && <Snippet text={post.summary} />}
      <Media post={post} />
      {why && (
        <p className="card-status why" role="note">
          {why}
        </p>
      )}
      {asking && (
        <ConfirmRow
          question="Delete this post?"
          detail="It won't come back."
          action="Delete"
          busy={busy}
          onConfirm={() => {
            setAsking(false);
            onDelete();
          }}
          onCancel={() => setAsking(false)}
        />
      )}
      <div className="card-buttons feed-foot">
        <button type="button" disabled={busy} onClick={onDiscuss}>
          Discuss
        </button>
        <button
          type="button"
          className="quiet"
          aria-label="Love"
          aria-pressed={post.loved}
          disabled={busy || post.loved}
          onClick={onLove}
        >
          <span aria-hidden="true">{post.loved ? "♥" : "♡"}</span> Love
        </button>
      </div>
    </li>
  );
}

export interface IdeaRowProps {
  idea: PulseIdea;
  busy: boolean;
  onDo: () => void;
  /** A PC health card's main button: open its options window. */
  onFix?: () => void;
  onMoreLike: () => void;
  onNotInterested: () => void;
}

/** One Ideas row: a clay icon, the title, its verdict and its grey "I can …"
 * line, then the same two buttons every card has. The main one is Let's do
 * it, or Fix it / Options on a PC health card; the second is Not interested.
 * A click on the row is the main button. More like this waits in the menu. */
export function IdeaRow({ idea, busy, onDo, onFix, onMoreLike, onNotInterested }: IdeaRowProps) {
  const [menu, setMenu] = useState(false);
  const corner = useDismiss<HTMLDivElement>(menu, () => setMenu(false));
  const pick = (f: () => void) => () => {
    setMenu(false);
    f();
  };
  const fix = idea.fix && onFix ? onFix : null;
  const main = fix ?? onDo;
  return (
    <li className="idea-row" aria-label={idea.title}>
      <button type="button" className="idea-main" disabled={busy} onClick={main}>
        <TopicIcon icon={idea.icon} fallback={IDEA_ICON} className="idea-icon" />
        <span className="idea-text">
          <span className="idea-title">{idea.title}</span>
          {idea.verdict && (
            <span className={`verdict ${idea.verdict.tone}`}>
              <span className="verdict-dot" aria-hidden="true" />
              {idea.verdict.line}
            </span>
          )}
          {idea.summary && <span className="idea-summary">{idea.summary}</span>}
        </span>
      </button>
      <div className="card-buttons idea-buttons">
        <button type="button" disabled={busy} onClick={main}>
          {fix ? idea.fix : "Let's do it"}
        </button>
        <button type="button" className="quiet" disabled={busy} onClick={onNotInterested}>
          Not interested
        </button>
      </div>
      <div className="feed-corner" ref={corner}>
        <button
          type="button"
          className="quiet icon-button"
          aria-label="More"
          aria-haspopup="menu"
          aria-expanded={menu}
          onClick={() => setMenu((m) => !m)}
        >
          ⋯
        </button>
        {menu && (
          <div className="feed-menu" role="menu">
            {fix && (
              <button type="button" role="menuitem" disabled={busy} onClick={pick(onDo)}>
                Ask about it
              </button>
            )}
            <button type="button" role="menuitem" disabled={busy} onClick={pick(onMoreLike)}>
              More like this
            </button>
          </div>
        )}
      </div>
    </li>
  );
}

/** Placeholder rows while posts load. */
export function SkeletonRows({ n = 3 }: { n?: number }) {
  return (
    <>
      {Array.from({ length: n }, (_, i) => (
        <li key={i} className="card post feed-card skeleton" aria-hidden="true">
          <div className="feed-head">
            <span className="topic-icon skeleton-block" />
            <span className="skeleton-line wide" />
          </div>
          <span className="skeleton-line" />
          <span className="skeleton-line short" />
        </li>
      ))}
    </>
  );
}

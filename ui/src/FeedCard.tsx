import { useEffect, useRef, useState, type MouseEvent, type ReactNode, type RefObject } from "react";
import { pulseImage, type PulseChip, type PulseIdea, type PulsePost } from "./api";
import { externalUrlOk, openExternal } from "./external";
import { Markdown } from "./Markdown";
import { VerdictLine } from "./PulseFix";
import { reducedMotion } from "./rubberBand";
import { ConfirmRow, ExternalLink, useDismiss, useModalFocus } from "./ui";

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
  // pre-line: a digest is a list, one line per item.
  return (
    <p className="snippet" style={{ whiteSpace: "pre-line" }}>
      {parts}
    </p>
  );
}

/** A card shows at most this many images: the lead, then two behind Show more. */
const IMAGES_MAX = 3;
/** Data URLs kept across remounts (a refresh, a tab switch), oldest out first. */
const IMAGE_CACHE_MAX = 48;
const imageCache = new Map<string, Promise<string>>();

/** One cached image as a data URL, asked of the app once per name. A failed
 * read is forgotten, so the next mount asks again. */
function loadImage(name: string): Promise<string> {
  const hit = imageCache.get(name);
  if (hit) {
    imageCache.delete(name);
    imageCache.set(name, hit);
    return hit;
  }
  const got = pulseImage(name);
  imageCache.set(name, got);
  got.catch(() => {
    if (imageCache.get(name) === got) imageCache.delete(name);
  });
  for (const old of imageCache.keys()) {
    if (imageCache.size <= IMAGE_CACHE_MAX) break;
    imageCache.delete(old);
  }
  return got;
}

/** A loaded image: its cached name, its data URL and its place on the card (1-based). */
interface Shot {
  name: string;
  src: string;
  n: number;
}

/** The images in `names` that loaded, in order, or null until they have.
 * Nothing is asked until `enabled`. `first` is the first name's place on the card. */
function useImages(names: string[], enabled: boolean, first: number): Shot[] | null {
  // `names` is a new array on every page load; its JSON only changes when a name does.
  const key = JSON.stringify(names);
  const [got, setGot] = useState<{ key: string; shots: Shot[] } | null>(null);
  useEffect(() => {
    if (!enabled) return;
    let live = true;
    const list: string[] = JSON.parse(key);
    Promise.all(
      list.map((name, i) =>
        loadImage(name).then(
          (src): Shot | null => (src ? { name, src, n: first + i } : null),
          () => null,
        ),
      ),
    ).then((all) => {
      if (live) setGot({ key, shots: all.filter((s): s is Shot => s !== null) });
    });
    return () => {
      live = false;
    };
  }, [key, enabled, first]);
  return got?.key === key ? got.shots : null;
}

/** True once `ref`'s element comes within 600 px of the screen, and from then
 * on. Without IntersectionObserver it is true at once. */
function useNearScreen(ref: RefObject<Element | null>, watch: boolean): boolean {
  const [near, setNear] = useState(() => typeof IntersectionObserver === "undefined");
  useEffect(() => {
    const el = ref.current;
    if (near || !watch || !el) return;
    const io = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) setNear(true);
      },
      { rootMargin: "600px 0px" },
    );
    io.observe(el);
    return () => io.disconnect();
  }, [ref, near, watch]);
  return near;
}

const MIME_EXT: Record<string, string> = { "image/jpeg": "jpg", "image/png": "png", "image/webp": "webp" };

/** The Download name: the cached name with the extension its bytes call for,
 * so a `.img` file saves as what it is. */
function downloadName(shot: Shot): string {
  const mime = /^data:([^;,]+)/.exec(shot.src)?.[1] ?? "";
  const ext = MIME_EXT[mime];
  if (!ext) return shot.name;
  const dot = shot.name.lastIndexOf(".");
  return `${dot > 0 ? shot.name.slice(0, dot) : shot.name}.${ext}`;
}

/** The host of a web link ("blog.rust-lang.org"), or "" when it is not one. */
function linkHost(link: string): string {
  if (!externalUrlOk(link)) return "";
  return new URL(link).hostname;
}

function ShotButton({ shot, onOpen }: { shot: Shot; onOpen: (shot: Shot) => void }) {
  return (
    <button
      type="button"
      className="media-item"
      aria-label={`Open image ${shot.n}`}
      onClick={() => onOpen(shot)}
    >
      <img src={shot.src} alt="" decoding="async" />
    </button>
  );
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
  const box = useRef<HTMLDivElement>(null);
  useModalFocus(box);
  useEffect(() => {
    close.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  return (
    <div ref={box} className="lightbox" role="dialog" aria-modal="true" aria-label="Image" onClick={onClose}>
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

/** The card's right column: its first image once the card nears the screen,
 * in a fixed 16:9 black box so nothing shifts. A card with a link and no
 * image shows its topic icon and the link's host instead; a card with neither
 * has no lead, so no artwork is made up. */
function Lead({ post, onOpen }: { post: PulsePost; onOpen: (shot: Shot) => void }) {
  const box = useRef<HTMLDivElement>(null);
  const lead = post.images.slice(0, 1);
  // Watch the lead box, not the card: the card's dwell watcher owns the <li>.
  const near = useNearScreen(box, lead.length > 0);
  const shots = useImages(lead, near, 1);
  const host = linkHost(post.link);
  const shot = shots?.[0];
  const loading = lead.length > 0 && shots === null;
  if (!shot && !loading && !host) return null;
  return (
    <div className="feed-lead" ref={box}>
      {shot ? (
        <ShotButton shot={shot} onOpen={onOpen} />
      ) : (
        !loading && (
          <div className="feed-tile" aria-hidden="true">
            <TopicIcon icon={post.icon} className="feed-tile-icon" />
            <span className="feed-tile-host">{host}</span>
          </div>
        )
      )}
    </div>
  );
}

/** Images 2 and 3, behind Show more; asked for only once the card is open. */
function Gallery({ post, onOpen }: { post: PulsePost; onOpen: (shot: Shot) => void }) {
  const shots = useImages(post.images.slice(1, IMAGES_MAX), true, 2);
  if (!shots?.length) return null;
  return (
    <div className="media gallery">
      {shots.map((shot) => (
        <ShotButton key={shot.name} shot={shot} onOpen={onOpen} />
      ))}
    </div>
  );
}

/** "Oct 26, 2026, 3:04 PM" for the menu. */
export function fullDate(ms: number): string {
  return new Date(ms).toLocaleString(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  });
}

/** The same row of buttons on every card, in one line: Like first (far
 * left), the card's own buttons in the middle, Not interested last (far
 * right). A like turns into a filled heart that stays; a second tap takes
 * it back. The heart pops once on the like tap, unless reduced motion is on.
 * Not interested drops the card immediately — no confirm. */
export function CardButtons({
  liked,
  busy,
  onLike,
  onDislike,
  children,
}: {
  liked: boolean;
  busy: boolean;
  onLike: () => void;
  onDislike: () => void;
  children?: ReactNode;
}) {
  const [popping, setPopping] = useState(false);
  // Heart shows while liked, or during the one-shot pop before liked lands.
  // The pop class drops as soon as liked is true (no effect); animationend
  // clears popping for idea rows where liked never sticks.
  const showHeart = liked || popping;
  const like = () => {
    if (liked) {
      setPopping(false);
      onLike();
      return;
    }
    if (!reducedMotion()) setPopping(true);
    onLike();
  };
  return (
    <div className="card-buttons card-row feed-foot" role="group" aria-label="Card actions">
      <button
        type="button"
        className={["quiet", "card-like", popping && !liked ? "card-like-pop" : ""]
          .filter(Boolean)
          .join(" ")}
        aria-label="Like"
        aria-pressed={liked}
        disabled={busy}
        onClick={like}
      >
        {showHeart ? (
          <span
            className="card-like-heart"
            aria-hidden="true"
            onAnimationEnd={() => setPopping(false)}
          >
            ♥
          </span>
        ) : (
          <>
            <span aria-hidden="true">♡</span> Like
          </>
        )}
      </button>
      {children}
      <button
        type="button"
        className="quiet card-not-interested"
        disabled={busy}
        onClick={onDislike}
      >
        Not interested
      </button>
    </div>
  );
}

export interface FeedCardProps {
  post: PulsePost;
  busy: boolean;
  /** Like, or take it back when the post is already liked. */
  onLove: () => void;
  onDiscuss: () => void;
  /** The post's source page opened in the browser, from the title or Read it. */
  onOpen?: () => void;
  /** The browser could not open the source page. */
  onOpenFailed?: (err: unknown) => void;
  /** A catalog upgrade card's Open in Settings: the model library, on that
   * model. Its Download button is the approval. */
  onOpenModel?: () => void;
  /** Not interested: off the page immediately, and fewer posts like it. */
  onDislike: () => void;
  /** The ⋯ menu's only item: why this card was made. */
  onWhy?: () => Promise<string>;
  /** Hidden until the next 9 AM. Shown when the card says it suits. */
  onSnooze?: () => void;
  /** Save: the post goes to the Saved page. Shown on source posts. */
  onSave?: () => void;
  /** A chip under the card other than Discuss: a fix, Ignore for now, or
   * Remind me later. */
  onChip?: (chip: PulseChip) => void;
  /** A weekly self-review card's Apply change, Revert or Save idea. */
  onReview?: () => void;
  /** A code-idea card's Copy as GitHub issue, in the ⋯ menu. */
  onIssue?: () => void;
  /** A Done for you card's Undo, after its confirm. */
  onUndo?: () => void;
  /** A Done for you card's Stop doing this. */
  onStop?: () => void;
}

/** Elements whose own click is not a click on the card body. */
const OWN_CLICK = "a, button, input, textarea, select, video, [role=menu], [role=dialog]";

/** One Feed post: icon, title (a link to the source when there is one),
 * where it is from and why, verdict, cited snippet and a digest's items, with
 * the lead image or a link tile beside them. Show more (or a click on the card
 * body) opens the full report, images 2 and 3, and why the card is here; a
 * self-review card starts open, so Apply never runs on text the user hasn't
 * seen. Then one row of buttons (Like, the card's chips or Discuss, Read it
 * or a model card's Open in Settings, Snooze when it suits the card, a
 * self-review card's Apply change, Revert or Save idea, Not interested). Not
 * interested drops the card immediately. The ⋯ menu holds "Why I created
 * this", plus Copy as GitHub issue on a code idea. */
export function FeedCard({
  post,
  busy,
  onLove,
  onDiscuss,
  onOpen,
  onOpenFailed,
  onOpenModel,
  onDislike,
  onWhy,
  onSnooze,
  onSave,
  onChip,
  onReview,
  onIssue,
  onUndo,
  onStop,
}: FeedCardProps) {
  const chips = post.chips ?? [];
  const [menu, setMenu] = useState(false);
  const [why, setWhy] = useState<string | null>(null);
  const [undoing, setUndoing] = useState(false);
  const openDone = post.done != null && !post.done.answered;
  const corner = useDismiss<HTMLDivElement>(menu, () => setMenu(false));
  // Per card; the card is keyed by post id, so it stays open across refreshes.
  const [open, setOpen] = useState(() => Boolean(post.review));
  const [shown, setShown] = useState<Shot | null>(null);
  const linked = externalUrlOk(post.link);
  const hasMore = Boolean(post.more || post.why) || post.images.length > 1;
  const regionId = `more-${post.id}`;
  async function askWhy() {
    setMenu(false);
    if (onWhy) setWhy(await onWhy().catch((err) => String(err)));
  }
  function openSource() {
    void openExternal(post.link).catch((err) => onOpenFailed?.(err));
    onOpen?.();
  }
  // A pointer click on the card's own text toggles it; links, buttons, the
  // menu, the lightbox and a text selection keep their own meaning. The
  // Show more button is the keyboard path.
  function clickBody(e: MouseEvent<HTMLLIElement>) {
    if (!hasMore) return;
    if (e.target instanceof Element && e.target.closest(OWN_CLICK)) return;
    if (window.getSelection()?.toString()) return;
    setOpen((o) => !o);
  }
  return (
    <li
      className={open ? "card post feed-card open" : "card post feed-card"}
      aria-label={post.title}
      data-card-id={post.id}
      onClick={clickBody}
    >
      <div className="feed-main">
        <div className="feed-head">
          <TopicIcon icon={post.icon} />
          {linked ? (
            <a
              className="feed-title"
              href={post.link}
              target="_blank"
              rel="noopener noreferrer"
              onClick={(e) => {
                e.preventDefault();
                openSource();
              }}
            >
              {post.title}
            </a>
          ) : (
            <div className="feed-title">{post.title}</div>
          )}
          <div className="feed-corner" ref={corner}>
            <span className="card-status" title={fullDate(post.createdAt)}>
              {post.ago}
            </span>
            {onWhy && (
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
            )}
            {menu && (
              <div className="feed-menu" role="menu">
                <button type="button" role="menuitem" onClick={() => void askWhy()}>
                  Why I created this
                </button>
                {post.issue && onIssue && (
                  <button
                    type="button"
                    role="menuitem"
                    onClick={() => {
                      setMenu(false);
                      onIssue();
                    }}
                  >
                    Copy as GitHub issue
                  </button>
                )}
              </div>
            )}
          </div>
        </div>
        <div className="feed-text">
          <p className="feed-source">
            {post.source}
            {post.because && ` · ${post.because}`}
          </p>
          <VerdictLine verdict={post.verdict} />
          {post.summary && <Snippet text={post.summary} />}
          {/* A digest's items are its body, so they stay out from behind Show more. */}
          {post.items?.length > 0 && (
            <div className="feed-report">
              <ul className="feed-items">
                {post.items.map((item) => (
                  <li key={item.cardId}>
                    <strong>{item.found}</strong>
                    {item.summary && ` ${item.summary}`}
                  </li>
                ))}
              </ul>
            </div>
          )}
          {hasMore && (
            <button
              type="button"
              className="quiet feed-more"
              aria-expanded={open}
              aria-controls={regionId}
              onClick={() => setOpen((o) => !o)}
            >
              {open ? "Show less" : "Show more"}
            </button>
          )}
        </div>
        <Lead post={post} onOpen={setShown} />
      </div>
      {hasMore && (
        <div
          id={regionId}
          className="feed-expanded"
          role="region"
          aria-label={`More about ${post.title}`}
          hidden={!open}
        >
          {open && (
            <>
              {post.more && (
                <div className="feed-report">
                  <Markdown text={post.more} />
                </div>
              )}
              <Gallery post={post} onOpen={setShown} />
              {post.why && (
                <p className="feed-why">
                  <span className="feed-why-label">Why you&apos;re seeing this</span>
                  {post.why}
                </p>
              )}
            </>
          )}
        </div>
      )}
      {shown && <Lightbox src={shown.src} name={downloadName(shown)} onClose={() => setShown(null)} />}
      {why && (
        <p className="card-status why" role="note">
          {why}
        </p>
      )}
      {undoing && openDone && (
        <ConfirmRow
          question="Turn it back on?"
          action="Undo"
          busy={busy}
          onConfirm={() => {
            setUndoing(false);
            onUndo?.();
          }}
          onCancel={() => setUndoing(false)}
        />
      )}
      <CardButtons liked={post.loved} busy={busy} onLike={onLove} onDislike={onDislike}>
        {chips.length === 0 ? (
          <button type="button" className="quiet" disabled={busy} onClick={onDiscuss}>
            Discuss
          </button>
        ) : (
          chips.map((chip) => (
            <button
              key={`${chip.act}-${chip.index}`}
              type="button"
              className="quiet"
              disabled={busy}
              onClick={() => (chip.act === "discuss" ? onDiscuss() : onChip?.(chip))}
            >
              {chip.label}
            </button>
          ))
        )}
        {post.review && onReview && (
          <button type="button" className="quiet" disabled={busy} onClick={onReview}>
            {post.review}
          </button>
        )}
        {openDone && onUndo && (
          <button
            type="button"
            className="quiet"
            disabled={busy}
            // A pointer click only: Enter or Space on a focused button does nothing.
            onClick={(e) => {
              if (e.detail > 0) setUndoing(true);
            }}
          >
            Undo
          </button>
        )}
        {openDone && onStop && (
          <button type="button" className="quiet" disabled={busy} onClick={onStop}>
            Stop doing this
          </button>
        )}
        {linked && onOpen && (
          <button type="button" className="quiet" disabled={busy} onClick={openSource}>
            Read it
          </button>
        )}
        {post.model && onOpenModel && (
          <button type="button" className="quiet" disabled={busy} onClick={onOpenModel}>
            Open in Settings
          </button>
        )}
        {post.save && onSave && (
          <button
            type="button"
            className="quiet"
            aria-pressed={post.saved}
            disabled={busy || post.saved}
            onClick={onSave}
          >
            {post.saved ? "Saved" : "Save"}
          </button>
        )}
        {post.snooze && onSnooze && (
          <button type="button" className="quiet" disabled={busy} onClick={onSnooze}>
            Snooze
          </button>
        )}
      </CardButtons>
    </li>
  );
}

export interface IdeaRowProps {
  idea: PulseIdea;
  busy: boolean;
  onDo: () => void;
  /** A skill idea's main button: save the skill. */
  onAddSkill?: () => void;
  /** A PC health card's main button: open its options window. */
  onFix?: () => void;
  /** Like, or take it back when the idea is already liked. */
  onMoreLike: () => void;
  /** Not interested: the idea goes immediately, with some of its topics' weight. */
  onNotInterested: () => void;
  /** A chip on a card that found something; Discuss goes to `onDiscuss`. */
  onChip?: (chip: PulseChip) => void;
  onDiscuss?: () => void;
}

/** One Ideas row, always fully open: a clay icon, the title, its verdict, its
 * grey "I can …" line and any full report, then the same row of buttons as
 * every card: Like, its own buttons (Let's do it, or Fix it / Options and
 * Ask about it on a PC health card, or the chips of a card that found
 * something), Not interested. A click on the row is the main button. */
export function IdeaRow({
  idea,
  busy,
  onDo,
  onAddSkill,
  onFix,
  onMoreLike,
  onNotInterested,
  onChip,
  onDiscuss,
}: IdeaRowProps) {
  const chips = idea.chips ?? [];
  const fix = idea.fix && onFix ? onFix : null;
  const addSkill = idea.skill && onAddSkill ? onAddSkill : null;
  const main = fix ?? addSkill ?? onDo;
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
      {idea.more && (
        <div className="feed-report">
          <Markdown text={idea.more} />
        </div>
      )}
      <CardButtons liked={idea.loved} busy={busy} onLike={onMoreLike} onDislike={onNotInterested}>
        {chips.length === 0 ? (
          <>
            <button type="button" className="quiet" disabled={busy} onClick={main}>
              {fix ? idea.fix : addSkill ? "Add skill" : "Let's do it"}
            </button>
            {fix && (
              <button type="button" className="quiet" disabled={busy} onClick={onDo}>
                Ask about it
              </button>
            )}
          </>
        ) : (
          chips.map((chip) => (
            <button
              key={`${chip.act}-${chip.index}`}
              type="button"
              className="quiet"
              disabled={busy}
              onClick={() => (chip.act === "discuss" ? onDiscuss?.() : onChip?.(chip))}
            >
              {chip.label}
            </button>
          ))
        )}
      </CardButtons>
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

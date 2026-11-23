import { useEffect, useRef, useState, type FormEvent } from "react";
import {
  getImagine,
  imagineCancel,
  imagineGenerate,
  imagineImage,
  imagineSignInPoll,
  imagineSignInStart,
  imagineSignOut,
  onWallNews,
  type ImagineKind,
  type ImaginePage,
  type ImagineTile,
} from "./api";
import { Lightbox } from "./FeedCard";
import { GrokSignIn, type SignInFlow } from "./GrokSignIn";
import { subscribeImagine } from "./imagineAsk";
import { PageHead } from "./ui";

/** What the shell says when there is no key, or Imagine refuses it. */
export const NEED_SIGNIN = "Sign in to Imagine first.";
export const SIGNIN_HINT = "Sign in with Grok here, or add an xAI API key in Settings → Account.";
export const IMAGINE_SIGN_IN = "Imagine sign-in";
export const IMAGINE_SIGN_IN_NOTE =
  "Use your Grok subscription for images and videos. Your sign-in is kept in this device's keychain.";
export const GALLERY_EMPTY = "Nothing yet. What you make, and the living wall's covers, show up here.";

/** Square first: the default. Values are what the shell accepts. */
export const ASPECTS: { value: string; label: string }[] = [
  { value: "1:1", label: "Square" },
  { value: "2:3", label: "Tall" },
  { value: "3:2", label: "Wide" },
  { value: "9:16", label: "Phone" },
  { value: "16:9", label: "Screen" },
];
export const COUNTS = [1, 2, 3, 4];

export function makingLine(kind: ImagineKind, count: number): string {
  if (kind === "video") return "Making a video… this can take a few minutes.";
  return count === 1 ? "Making 1 image…" : `Making ${count} images…`;
}

/** The status line when a living wall cover lands. */
export function wallLine(title: string): string {
  return `New cover on the wall: ${title}`;
}

export function paintingLine(title: string): string {
  return `Painting a wall cover: ${title}`;
}

export const WALL_RETRY = "The wall cover didn't come. It tries again in 15 minutes.";

/** The shell's refusal while another run holds Imagine (imagine.rs BUSY). */
export const IMAGINE_BUSY = "Imagine is already making something. Stop it or wait for it.";

function madeLine(made: number, note: string): string {
  const line = made === 1 ? "Made 1." : `Made ${made}.`;
  return note ? `${line} ${note}` : line;
}

/** Tile pictures as data URLs, loaded once per name. Videos load on open. */
function useThumb(tile: ImagineTile): string {
  const [src, setSrc] = useState("");
  useEffect(() => {
    if (tile.video) return;
    let live = true;
    imagineImage(tile.name).then(
      (url) => live && setSrc(url),
      () => {},
    );
    return () => {
      live = false;
    };
  }, [tile.name, tile.video]);
  return src;
}

function Tile({ tile, onOpen }: { tile: ImagineTile; onOpen: () => void }) {
  const src = useThumb(tile);
  const label = tile.title || tile.prompt || "Imagine result";
  return (
    <li className={tile.tall ? "imagine-tile tall" : "imagine-tile"}>
      <button type="button" className="media-item" aria-label={`Open ${label}`} onClick={onOpen}>
        {tile.video ? (
          <span className="imagine-video" aria-hidden="true">
            ▶ Video
          </span>
        ) : src ? (
          <img src={src} alt="" />
        ) : (
          <span className="imagine-video" aria-hidden="true" />
        )}
      </button>
      <span className="imagine-caption">{label}</span>
    </li>
  );
}

/** Opens one tile: its file loads now, then the lightbox shows it. */
function Opened({ tile, onClose }: { tile: ImagineTile; onClose: () => void }) {
  const [src, setSrc] = useState("");
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    imagineImage(tile.name).then(
      (url) => live && setSrc(url),
      (err) => live && setError(String(err)),
    );
    return () => {
      live = false;
    };
  }, [tile.name]);
  if (error) {
    return (
      <p className="error" role="alert">
        {error}
      </p>
    );
  }
  if (!src) return null;
  return <Lightbox src={src} name={tile.name} video={tile.video} onClose={onClose} />;
}

/** Images, or a short video when chosen, from a prompt. The gallery is the
 * cabin's Imagine wall, so both show the same results. */
export function Imagine() {
  const [page, setPage] = useState<ImaginePage | null>(null);
  const [prompt, setPrompt] = useState("");
  const [aspect, setAspect] = useState("1:1");
  const [count, setCount] = useState(1);
  const [kind, setKind] = useState<ImagineKind>("image");
  const [busy, setBusy] = useState(false);
  /** The living wall cover being painted now, by its title. */
  const [wall, setWall] = useState<string | null>(null);
  // The wall's news must not write over a Make's own status line.
  const making = useRef(false);
  useEffect(() => {
    making.current = busy;
  }, [busy]);
  /** A Make or a wall cover holds Imagine's one run: Stop shows. */
  const working = busy || wall !== null;
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState<ImagineTile | null>(null);
  const promptBox = useRef<HTMLTextAreaElement>(null);

  // `/imagine` and `/imagine-video`: the page opens on that kind, and a
  // prompt starts the same make the form does.
  useEffect(() => {
    return subscribeImagine((job) => {
      setKind(job.kind);
      setPrompt(job.prompt);
      const text = job.prompt.trim();
      if (!text) {
        promptBox.current?.focus();
        return;
      }
      setBusy(true);
      setError(null);
      setStatus(makingLine(job.kind, 1));
      imagineGenerate(text, "1:1", 1, job.kind).then(
        (made) => {
          setPage(made.page);
          setStatus(madeLine(made.made, made.note));
          setBusy(false);
        },
        (err) => {
          setStatus(null);
          setError(String(err));
          setBusy(false);
        },
      );
    });
  }, []);

  useEffect(() => {
    getImagine().then(setPage, (err) => setError(String(err)));
  }, []);

  // The living wall painting while the page is open: its line, Stop (the
  // run is Imagine's one run), then the gallery again when it lands.
  useEffect(() => {
    let live = true;
    const say = (line: string) => {
      if (!making.current) setStatus(line);
    };
    const off = onWallNews((news) => {
      if (news.state === "painting") {
        setWall(news.title);
        say(paintingLine(news.title));
        return;
      }
      setWall(null);
      // A Make refused while the cover held Imagine can go now.
      setError((now) => (now === IMAGINE_BUSY ? null : now));
      if (news.state === "failed") {
        say(WALL_RETRY);
        return;
      }
      getImagine().then(
        (next) => {
          if (!live) return;
          setPage(next);
          say(wallLine(news.title));
        },
        () => {},
      );
    });
    return () => {
      live = false;
      void off.then((unlisten) => unlisten());
    };
  }, []);

  /** Imagine's own sign-in, shown on this page as the cabin did. */
  const flow: SignInFlow = {
    title: IMAGINE_SIGN_IN,
    note: IMAGINE_SIGN_IN_NOTE,
    signedOut: "Signed out of Imagine.",
    start: imagineSignInStart,
    poll: imagineSignInPoll,
    done: async () => {
      setPage(await getImagine());
      setError(null);
    },
    signOut: async () => setPage(await imagineSignOut()),
  };

  async function make(event: FormEvent) {
    event.preventDefault();
    if (working || !prompt.trim()) return;
    const n = kind === "video" ? 1 : count;
    setBusy(true);
    setError(null);
    setStatus(makingLine(kind, n));
    try {
      const made = await imagineGenerate(prompt.trim(), aspect, n, kind);
      setPage(made.page);
      setStatus(madeLine(made.made, made.note));
    } catch (err) {
      setStatus(null);
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  /** The cabin's + New project: a fresh start. The prompt, the last result
   * line and any error clear, and the prompt takes the cursor. The gallery
   * stays, as the cabin's wall does. */
  function newProject() {
    setPrompt("");
    setStatus(null);
    setError(null);
    setOpen(null);
    promptBox.current?.focus();
  }

  function stop() {
    void imagineCancel().catch(() => {});
  }

  const signIn = page !== null && !page.ready;
  const needsSignIn = signIn || error === NEED_SIGNIN;
  // The sign-in shows when it is needed, and while Imagine's own is there so
  // it can be signed out. A chat sign-in or a key alone needs nothing here.
  const showSignIn = page !== null && page.canSignIn && (needsSignIn || page.account !== null);
  return (
    <section className="imagine" aria-label="Imagine">
      <PageHead title="Imagine" />
      <div className="card-buttons">
        <button type="button" className="quiet" onClick={newProject}>
          + New project
        </button>
      </div>
      <form className="card imagine-bar" onSubmit={(e) => void make(e)}>
        <label className="imagine-prompt">
          <span>What to make</span>
          <textarea
            ref={promptBox}
            value={prompt}
            rows={3}
            placeholder="A snowy cabin at night, one window lit"
            onChange={(e) => setPrompt(e.target.value)}
          />
        </label>
        <div className="chips">
          <div className="chip-group" role="group" aria-label="Kind">
            {(["image", "video"] as const).map((k) => (
              <button
                key={k}
                type="button"
                className="chip"
                aria-pressed={kind === k}
                disabled={working}
                onClick={() => setKind(k)}
              >
                {k === "image" ? "Image" : "Video"}
              </button>
            ))}
          </div>
          <div className="chip-group" role="group" aria-label="Aspect">
            {ASPECTS.map((a) => (
              <button
                key={a.value}
                type="button"
                className="chip"
                aria-pressed={aspect === a.value}
                title={a.value}
                disabled={working}
                onClick={() => setAspect(a.value)}
              >
                {a.label}
              </button>
            ))}
          </div>
          {kind === "image" && (
            <div className="chip-group" role="group" aria-label="How many">
              {COUNTS.map((n) => (
                <button
                  key={n}
                  type="button"
                  className="chip"
                  aria-pressed={count === n}
                  disabled={working}
                  onClick={() => setCount(n)}
                >
                  {n}
                </button>
              ))}
            </div>
          )}
        </div>
        <div className="card-buttons">
          {working ? (
            <button type="button" onClick={stop}>
              Stop
            </button>
          ) : (
            <button type="submit" disabled={!prompt.trim() || signIn}>
              Make
            </button>
          )}
        </div>
        {status && (
          <p className="status" role="status" aria-busy={working}>
            {status}
          </p>
        )}
        {needsSignIn ? (
          <div className="note warn" role="alert">
            <strong>{NEED_SIGNIN}</strong> {SIGNIN_HINT}
          </div>
        ) : (
          error && (
            <p className="error" role="alert">
              {error}
            </p>
          )
        )}
      </form>
      {showSignIn && (
        <div className="card">
          <GrokSignIn signedIn={page.account} flow={flow} />
        </div>
      )}
      {page && page.tiles.length === 0 && <p className="empty">{GALLERY_EMPTY}</p>}
      {page && page.tiles.length > 0 && (
        <ul className="imagine-gallery" aria-label="Gallery">
          {page.tiles.map((tile) => (
            <Tile key={tile.id} tile={tile} onOpen={() => setOpen(tile)} />
          ))}
        </ul>
      )}
      {open && <Opened tile={open} onClose={() => setOpen(null)} />}
    </section>
  );
}

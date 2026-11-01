import { useEffect, useState, type FormEvent } from "react";
import {
  getImagine,
  imagineCancel,
  imagineGenerate,
  imagineImage,
  type ImagineKind,
  type ImaginePage,
  type ImagineTile,
} from "./api";
import { Lightbox } from "./FeedCard";

/** What the shell says when there is no key, or Imagine refuses it. */
export const NEED_SIGNIN = "Sign in to Imagine in the cabin.";
export const SIGNIN_HINT = "Or sign in with Grok or add an xAI API key in Settings → Account.";
export const GALLERY_EMPTY = "Nothing yet. What you make shows up here and on the cabin's wall.";

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
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState<ImagineTile | null>(null);

  useEffect(() => {
    getImagine().then(setPage, (err) => setError(String(err)));
  }, []);

  async function make(event: FormEvent) {
    event.preventDefault();
    if (busy || !prompt.trim()) return;
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

  function stop() {
    void imagineCancel().catch(() => {});
  }

  const signIn = page !== null && !page.ready;
  const needsSignIn = signIn || error === NEED_SIGNIN;
  return (
    <section className="imagine" aria-label="Imagine">
      <form className="card imagine-bar" onSubmit={(e) => void make(e)}>
        <label className="imagine-prompt">
          <span>What to make</span>
          <textarea
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
                disabled={busy}
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
                disabled={busy}
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
                  disabled={busy}
                  onClick={() => setCount(n)}
                >
                  {n}
                </button>
              ))}
            </div>
          )}
        </div>
        <div className="card-buttons">
          {busy ? (
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
          <p className="status" role="status" aria-busy={busy}>
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

import { openSaved, revealSaved, type SavedFile } from "./api";
import { StatusLine, useNote } from "./ui";

/** A chat export on disk: its name (a click opens it), Open, Show in folder
 * and Copy path. The buttons send the file's token, never its path. */
export function SavedCard({ saved, onClose }: { saved: SavedFile; onClose?: () => void }) {
  const { note, ok, fail, clear } = useNote();

  async function run(step: () => Promise<void>) {
    clear();
    try {
      await step();
    } catch (err) {
      fail(err);
    }
  }

  async function copy() {
    try {
      await navigator.clipboard.writeText(saved.path);
      ok("Copied the file's path.");
    } catch {
      fail("Couldn't copy the path.");
    }
  }

  const open = () => void run(() => openSaved(saved.token));
  return (
    <div className="card notice saved-card" role="status" aria-label={saved.line}>
      <button type="button" className="link card-title" title={saved.path} onClick={open}>
        {saved.line}
      </button>
      <div className="card-buttons">
        <button type="button" onClick={open}>
          Open
        </button>
        <button type="button" className="quiet" onClick={() => void run(() => revealSaved(saved.token))}>
          {saved.revealLabel}
        </button>
        <button type="button" className="quiet" onClick={() => void copy()}>
          Copy path
        </button>
        {onClose && (
          <button type="button" className="quiet" onClick={onClose}>
            Close
          </button>
        )}
      </div>
      <StatusLine note={note} />
    </div>
  );
}

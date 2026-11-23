import { useEffect, useState } from "react";
import { PRODUCT_NAME } from "./brand";
import {
  getLocalModelNews,
  localModelNewsCheckNow,
  setLocalModelNews,
  type LocalModelNews,
} from "./api";
import { Toggle } from "./Toggle";

export const LOCAL_MODELS_LABEL = "Suggest new local models that fit this PC";
export const LOCAL_MODELS_HELP = `Once a week, from this PC's memory and huihui-ai's abliterated builds on Hugging Face. Only models in ${PRODUCT_NAME}'s checked catalog download, from Settings.`;
/** The check's own note while it runs (local_model_checks.rs CHECKING). */
export const CHECKING = "Checking for new local models…";
/** How often a running check is asked how it went. */
export const POLL_MS = 2000;

/** Settings → Pulse: the opt-in Local models source. */
export function LocalModelsSource() {
  const [page, setPage] = useState<LocalModelNews | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    getLocalModelNews().then(setPage, (err: unknown) => setError(String(err)));
  }, []);

  // A check runs off the page: read its note again until it has finished.
  const checking = page?.on === true && page.note === CHECKING;
  useEffect(() => {
    if (!checking) return;
    const timer = setTimeout(() => {
      getLocalModelNews().then(setPage, (err: unknown) => setError(String(err)));
    }, POLL_MS);
    return () => clearTimeout(timer);
  }, [checking, page]);

  async function run(work: () => Promise<LocalModelNews>) {
    setBusy(true);
    setError(null);
    try {
      setPage(await work());
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="steam-pulse-sources" role="group" aria-label="Local models">
      <h2>Local models</h2>
      {error && (
        <p className="note" role="alert">
          {error}
        </p>
      )}
      {page === null && !error && <p className="empty">Loading…</p>}
      {page && (
        <>
          <Toggle checked={page.on} disabled={busy} onChange={(next) => void run(() => setLocalModelNews(next))}>
            {LOCAL_MODELS_LABEL}
          </Toggle>
          {page.on && (
            <div className="steam-source-detail">
              <p className="note">{LOCAL_MODELS_HELP}</p>
              <div className="labs-row">
                <button
                  type="button"
                  className="quiet"
                  disabled={busy || checking}
                  onClick={() => void run(localModelNewsCheckNow)}
                >
                  Check now
                </button>
              </div>
              {page.note && (
                <p className="note" role="status">
                  {page.note}
                </p>
              )}
            </div>
          )}
        </>
      )}
    </div>
  );
}

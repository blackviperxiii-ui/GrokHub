import { useCallback, useEffect, useState } from "react";
import {
  getUpdatesPage,
  installShellUpdate,
  runUpdateOffer,
  type UpdateGroup,
  type UpdateItem,
  type UpdateOffer,
  type UpdatesPage,
} from "./api";
import { StatusLine, useNote } from "./ui";

/** Offers that change what's installed: the page reads again after them. */
const RE_READ = new Set<UpdateOffer>(["updateSystem", "updateApps", "updateDrivers", "rollBackDrivers"]);

/** How many of a group's items show before "and N more". */
export const SHOWN = 6;

/** "linux 6.10.1 → 6.10.2", or just the name when the source gives no versions. */
export function itemLine(item: UpdateItem): string {
  if (item.from && item.to) return `${item.name} ${item.from} → ${item.to}`;
  if (item.to) return `${item.name} ${item.to}`;
  return item.name;
}

/** Home's Updates & Drivers page: what waits to update on this PC, one group
 * per kind, one offer each. Nothing runs until a tap. */
export function UpdatesDrivers() {
  const [page, setPage] = useState<UpdatesPage | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const { note, ok, fail } = useNote();

  useEffect(() => {
    getUpdatesPage().then(setPage, fail);
  }, [fail]);

  const read = useCallback(() => {
    setPage(null);
    getUpdatesPage().then(setPage, fail);
  }, [fail]);

  async function take(group: UpdateGroup) {
    const offer = group.offer;
    if (!offer || offer.held) return;
    setBusy(group.kind);
    try {
      if (offer.offer === "updateLoadout") {
        // Downloads, verifies and restarts into the new version.
        await installShellUpdate();
      } else {
        ok(await runUpdateOffer(offer.offer));
        if (RE_READ.has(offer.offer)) read();
      }
    } catch (err) {
      fail(err);
    } finally {
      setBusy(null);
    }
  }

  return (
    <section className="updates-page" aria-label="Updates & Drivers">
      <div className="card">
        <div className="card-title">{page?.headline ?? "Checking what's waiting…"}</div>
        {page?.driver && <div className="card-detail-line">{page.driver}</div>}
        <div className="card-buttons">
          <button type="button" className="quiet" onClick={read} disabled={page === null}>
            Check again
          </button>
        </div>
      </div>
      {page?.groups.map((g) => (
        <div className="card" key={g.kind} aria-label={g.title}>
          <div className="card-title">{g.title}</div>
          <div className="card-status">{g.line}</div>
          {g.items.length > 0 && (
            <ul className="card-list">
              {g.items.slice(0, SHOWN).map((item) => (
                <li key={item.name}>{itemLine(item)}</li>
              ))}
              {g.items.length > SHOWN && <li>and {g.items.length - SHOWN} more</li>}
            </ul>
          )}
          {g.offer && (
            <div className="card-buttons">
              <button
                type="button"
                className="primary"
                disabled={busy !== null || g.offer.held !== null}
                onClick={() => void take(g)}
              >
                {busy === g.kind ? "Working…" : g.offer.label}
              </button>
              {g.offer.held && <span className="card-detail-line">{g.offer.held}</span>}
            </div>
          )}
        </div>
      ))}
      <StatusLine note={note} />
    </section>
  );
}

import { useEffect, useState } from "react";
import { vstackSyncNow, vstackSyncStatus, type VstackStatus } from "./api";

/** Synced tag (or bundled version), commit, and skill count on one line. */
export function vstackStatusLine(status: VstackStatus): string {
  const tag = status.tag ?? status.version;
  const commit = status.commit.slice(0, 12);
  const source = status.source === "synced" ? "synced" : "bundled";
  return `${source} ${tag} · ${commit} · ${status.skillCount} skills`;
}

/** vstack sync status and Sync now. `onSynced` reloads the skill list. */
export function VstackSyncBar({ onSynced }: { onSynced: () => void }) {
  const [status, setStatus] = useState<VstackStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);

  useEffect(() => {
    let gone = false;
    vstackSyncStatus().then(
      (next) => {
        if (!gone && next?.commit) setStatus(next);
      },
      (err) => {
        if (!gone) setNote(String(err));
      },
    );
    return () => {
      gone = true;
    };
  }, []);

  async function syncNow() {
    setBusy(true);
    setNote(null);
    try {
      const next = await vstackSyncNow();
      if (next?.commit) setStatus(next);
      onSynced();
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="vstack-sync" role="group" aria-label="vstack sync">
      {status && <p className="note">{vstackStatusLine(status)}</p>}
      <button type="button" disabled={busy} onClick={() => void syncNow()}>
        {busy ? "Syncing…" : "Sync now"}
      </button>
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
    </div>
  );
}

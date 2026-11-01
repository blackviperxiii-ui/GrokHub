import { useEffect, useState } from "react";
import {
  addCatalogConnector,
  addConnectorConfig,
  answerConnectorKey,
  connectorCatalog,
  saveConnectorKey,
  type CatalogRow,
  type ConnectorsChanged,
  type KeyCard,
} from "./api";
import { StatusLine, useNote } from "./ui";

export const KEY_CARD_DETAIL =
  "It's sealed on this computer and only this connector gets it. Nothing is saved until you choose Save.";

export const PASTE_HINT = '{"mcpServers": {"name": {"command": "npx", "args": ["…"]}}}';

/** How a catalog connector signs in once it's added. */
export function authLine(row: CatalogRow): string {
  switch (row.auth) {
    case "key":
      return "Needs an API key";
    case "oauth":
      return "Signs in through your browser";
    case "google":
      return "Set up through Google Cloud";
    case "own":
      return "Signs in by itself the first time it runs";
    default:
      return "Works as soon as it's added";
  }
}

/** The Add sheet: the catalog with search, a key field for key connectors,
 * and Paste config. Every change hands the new rows to `onChanged`. */
export function ConnectorAdd({
  onChanged,
  onClose,
}: {
  onChanged: (changed: ConnectorsChanged) => void;
  onClose: () => void;
}) {
  const [tab, setTab] = useState<"catalog" | "paste">("catalog");
  const [rows, setRows] = useState<CatalogRow[] | null>(null);
  const [query, setQuery] = useState("");
  const [paste, setPaste] = useState("");
  const [keys, setKeys] = useState<Record<string, string>>({});
  const [card, setCard] = useState<KeyCard | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const { note, ok, fail } = useNote();

  async function load(refresh: boolean) {
    if (refresh) setBusy("refresh");
    try {
      const view = await connectorCatalog(refresh);
      setRows(view.rows);
      if (view.note) ok(view.note);
    } catch (err) {
      fail(err);
    } finally {
      if (refresh) setBusy(null);
    }
  }

  // The first read of the catalog, when the sheet opens.
  useEffect(() => {
    let live = true;
    connectorCatalog(false).then(
      (view) => {
        if (!live) return;
        setRows(view.rows);
        if (view.note) ok(view.note);
      },
      (err) => {
        if (live) fail(err);
      },
    );
    return () => {
      live = false;
    };
  }, [ok, fail]);

  async function run(label: string, step: () => Promise<ConnectorsChanged>) {
    setBusy(label);
    try {
      const changed = await step();
      onChanged(changed);
      ok(changed.status);
      const names = new Set(changed.rows.map((r) => r.name));
      setRows((list) => list?.map((r) => ({ ...r, installed: names.has(r.id) })) ?? list);
      return true;
    } catch (err) {
      fail(err);
      return false;
    } finally {
      setBusy(null);
    }
  }

  async function startKey(row: CatalogRow) {
    const key = keys[row.id] ?? "";
    setKeys((all) => ({ ...all, [row.id]: "" }));
    setBusy(row.id);
    try {
      setCard(await saveConnectorKey(row.id, key));
    } catch (err) {
      fail(err);
    } finally {
      setBusy(null);
    }
  }

  async function answer(approve: boolean) {
    if (!card) return;
    const id = card.id;
    setCard(null);
    await run(card.name, () => answerConnectorKey(id, approve));
  }

  const needle = query.trim().toLowerCase();
  const shown = (rows ?? []).filter(
    (r) =>
      !needle ||
      [r.name, r.publisher, r.category, r.description, r.id].some((f) =>
        f.toLowerCase().includes(needle),
      ),
  );

  return (
    <div className="card connector-add" role="dialog" aria-label="Add a connector">
      <div className="card-buttons" role="tablist" aria-label="How to add">
        <button
          type="button"
          role="tab"
          aria-selected={tab === "catalog"}
          className={tab === "catalog" ? "" : "quiet"}
          onClick={() => setTab("catalog")}
        >
          Catalog
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={tab === "paste"}
          className={tab === "paste" ? "" : "quiet"}
          onClick={() => setTab("paste")}
        >
          Paste config
        </button>
        <button type="button" className="quiet" onClick={onClose}>
          Close
        </button>
      </div>
      <StatusLine note={note} />
      {card && (
        <div className="card-buttons confirm-row" role="group" aria-label={`Save the ${card.name} key?`}>
          <span>
            Save the {card.name} key? {KEY_CARD_DETAIL}
          </span>
          <button type="button" onClick={() => void answer(true)}>
            Save
          </button>
          <button type="button" className="quiet" onClick={() => void answer(false)}>
            Deny
          </button>
        </div>
      )}
      {tab === "catalog" ? (
        <>
          <div className="card-buttons">
            <input
              aria-label="Search connectors"
              placeholder="Search connectors"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
            <button
              type="button"
              className="quiet"
              disabled={busy !== null}
              onClick={() => void load(true)}
            >
              {busy === "refresh" ? "Refreshing…" : "Refresh"}
            </button>
          </div>
          {rows === null && <p className="empty">Loading the catalog…</p>}
          <ul className="rows catalog">
            {shown.map((row) => (
              <li key={row.id} className="card connector" aria-label={row.name}>
                <div className="card-title">{row.name}</div>
                <div className="card-status">
                  {row.publisher} · {row.category}
                  {row.installed && <span className="badge">Added</span>}
                </div>
                <div className="card-detail-line">{row.description}</div>
                <div className="card-status">{authLine(row)}</div>
                <div className="card-buttons">
                  {!row.installed && (
                    <button
                      type="button"
                      disabled={busy !== null}
                      onClick={() => void run(row.id, () => addCatalogConnector(row.id))}
                    >
                      {busy === row.id ? "Adding…" : "Add"}
                    </button>
                  )}
                  {row.installed && row.auth === "key" && (
                    <>
                      <input
                        type="password"
                        autoComplete="off"
                        aria-label={`${row.name} API key`}
                        placeholder={row.keyHint || "API key"}
                        value={keys[row.id] ?? ""}
                        onChange={(e) => setKeys((all) => ({ ...all, [row.id]: e.target.value }))}
                      />
                      <button
                        type="button"
                        disabled={busy !== null || card !== null || !(keys[row.id] ?? "").trim()}
                        onClick={() => void startKey(row)}
                      >
                        Save key
                      </button>
                    </>
                  )}
                </div>
              </li>
            ))}
            {rows !== null && shown.length === 0 && <li className="note">No connector matches.</li>}
          </ul>
        </>
      ) : (
        <div className="paste-config">
          <textarea
            aria-label="mcpServers config"
            placeholder={PASTE_HINT}
            rows={6}
            value={paste}
            onChange={(e) => setPaste(e.target.value)}
          />
          <p className="note">Paste only configs you trust: each server runs or connects as you.</p>
          <div className="card-buttons">
            <button
              type="button"
              disabled={busy !== null || !paste.trim()}
              onClick={() =>
                void run("paste", () => addConnectorConfig(paste)).then((done) => {
                  if (done) setPaste("");
                })
              }
            >
              {busy === "paste" ? "Adding…" : "Add from config"}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

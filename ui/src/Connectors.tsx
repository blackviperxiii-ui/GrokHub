import { useEffect, useState } from "react";
import {
  checkConnectors,
  disconnectConnector,
  listConnectors,
  restartConnector,
  signOutConnector,
  type ConnectorRow,
  type ConnectorsChanged,
} from "./api";

export const DISCONNECT_DETAIL =
  "Its entry leaves your connections, and its sign-in and saved token are deleted.";

export const SIGN_IN_NOTE = "Needs a sign-in. Sign in from the GrokHub cabin for now.";

export function statusLine(row: ConnectorRow): string {
  switch (row.status) {
    case "connected":
      return `Connected · ${row.tools} ${row.tools === 1 ? "tool" : "tools"}`;
    case "error":
      return "Not connected";
    case "stopped":
      return "Stopped";
    case "disabled":
      return "Off";
    default:
      return "Not checked";
  }
}

/** The MCP servers in `mcp.json`, shared with the cabin and chat. */
export function Connectors() {
  const [rows, setRows] = useState<ConnectorRow[] | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [asking, setAsking] = useState<string | null>(null);

  useEffect(() => {
    listConnectors().then(setRows, (err) => setNote(String(err)));
  }, []);

  async function check() {
    setBusy("*");
    try {
      setRows(await checkConnectors());
      setNote(null);
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(null);
    }
  }

  async function act(name: string, step: () => Promise<ConnectorsChanged>) {
    setBusy(name);
    setAsking(null);
    try {
      const changed = await step();
      setRows(changed.rows);
      setNote(changed.status);
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(null);
    }
  }

  return (
    <section className="connectors" aria-label="Connectors">
      <div className="card-buttons">
        <button type="button" disabled={busy !== null || !rows?.length} onClick={() => void check()}>
          {busy === "*" ? "Checking…" : "Check all"}
        </button>
      </div>
      <p className="note">
        These are the MCP servers in mcp.json, the same ones the cabin and chat use. Add new
        ones from the cabin's Connectors page.
      </p>
      {note && (
        <p className="status" role="status">
          {note}
        </p>
      )}
      {rows === null && !note && <p className="empty">Loading connectors…</p>}
      {rows?.length === 0 && <p className="empty">No connectors yet.</p>}
      <ul className="rows">
        {rows?.map((row) => (
          <li key={row.name} className="card connector" aria-label={row.name}>
            <div className="card-title">{row.name}</div>
            <div className="card-status">{statusLine(row)}</div>
            <div className="card-detail-line">{row.detail}</div>
            {row.account && <div className="card-status">{row.account}</div>}
            {row.needsSignIn && <div className="card-status">{SIGN_IN_NOTE}</div>}
            {row.error && <div className="card-health">{row.error}</div>}
            {asking === row.name ? (
              <div className="card-buttons" role="group" aria-label={`Disconnect ${row.name}?`}>
                <span>
                  Disconnect {row.name}? {DISCONNECT_DETAIL}
                </span>
                <button
                  type="button"
                  className="danger"
                  disabled={busy !== null}
                  onClick={() => void act(row.name, () => disconnectConnector(row.name))}
                >
                  Disconnect
                </button>
                <button type="button" className="quiet" onClick={() => setAsking(null)}>
                  Keep
                </button>
              </div>
            ) : (
              <div className="card-buttons">
                {row.status !== "disabled" && (
                  <button
                    type="button"
                    disabled={busy !== null}
                    onClick={() => void act(row.name, () => restartConnector(row.name))}
                  >
                    {busy === row.name ? "Reconnecting…" : "Reconnect"}
                  </button>
                )}
                {row.account && (
                  <button
                    type="button"
                    className="quiet"
                    disabled={busy !== null}
                    onClick={() => void act(row.name, () => signOutConnector(row.name))}
                  >
                    Sign out
                  </button>
                )}
                {row.canDisconnect && (
                  <button
                    type="button"
                    className="quiet danger"
                    disabled={busy !== null}
                    onClick={() => setAsking(row.name)}
                  >
                    Disconnect…
                  </button>
                )}
              </div>
            )}
          </li>
        ))}
      </ul>
    </section>
  );
}

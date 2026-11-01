import { useEffect, useState } from "react";
import {
  checkConnectors,
  disconnectConnector,
  listConnectors,
  restartConnector,
  setConnectorEnabled,
  signInConnector,
  signOutConnector,
  type ConnectorRow,
  type ConnectorsChanged,
} from "./api";
import { PRODUCT_NAME } from "./brand";
import { ConnectorAdd } from "./ConnectorAdd";
import { ConfirmRow, StatusLine, useNote } from "./ui";

export const DISCONNECT_DETAIL =
  "Its entry leaves your connections, and its sign-in and saved token are deleted.";

export const CONNECTORS_EMPTY = "No connectors yet. Add one from the catalog or paste a config.";

export const REMOTE_EMPTY = `No connectors yet. Add them in ${PRODUCT_NAME} on your computer.`;

export const SIGN_IN_NOTE = "Needs a sign-in.";

export const REMOTE_NOTE = `Add, sign in and switch connectors in ${PRODUCT_NAME} on your computer.`;

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

/** The MCP servers in `mcp.json`, shared with the cabin and chat. The web
 * remote lists and checks them; adding, signing in and the switch stay on
 * the computer. */
export function Connectors({ remote = false }: { remote?: boolean }) {
  const [rows, setRows] = useState<ConnectorRow[] | null>(null);
  const { note, ok, fail, clear } = useNote();
  const [busy, setBusy] = useState<string | null>(null);
  const [asking, setAsking] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);

  useEffect(() => {
    listConnectors().then(setRows, fail);
  }, [fail]);

  async function check() {
    setBusy("*");
    try {
      setRows(await checkConnectors());
      clear();
    } catch (err) {
      fail(err);
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
      ok(changed.status);
    } catch (err) {
      fail(err);
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
        {!remote && (
          <button
            type="button"
            className="quiet"
            aria-expanded={adding}
            onClick={() => setAdding((open) => !open)}
          >
            Add…
          </button>
        )}
      </div>
      <p className="note">
        These are the MCP servers in mcp.json, the same ones the cabin and chat use.
        {remote ? ` ${REMOTE_NOTE}` : ""}
      </p>
      {adding && !remote && (
        <ConnectorAdd
          onChanged={(changed) => setRows(changed.rows)}
          onClose={() => setAdding(false)}
        />
      )}
      <StatusLine note={note} />
      {rows === null && !note && <p className="empty">Loading connectors…</p>}
      {rows?.length === 0 && <p className="empty">{remote ? REMOTE_EMPTY : CONNECTORS_EMPTY}</p>}
      <ul className="rows">
        {rows?.map((row) => (
          <li key={row.name} className="card connector" aria-label={row.name}>
            <div className="card-title">
              {row.name}
              {!remote && row.canDisconnect && (
                <label className="switch">
                  <input
                    type="checkbox"
                    role="switch"
                    aria-label={`${row.name} on`}
                    checked={row.enabled}
                    disabled={busy !== null}
                    onChange={(e) => {
                      const on = e.target.checked;
                      void act(row.name, () => setConnectorEnabled(row.name, on));
                    }}
                  />
                </label>
              )}
            </div>
            <div className="card-status">{statusLine(row)}</div>
            <div className="card-detail-line">{row.detail}</div>
            {row.account && <div className="card-status">{row.account}</div>}
            {row.needsSignIn && <div className="card-status">{SIGN_IN_NOTE}</div>}
            {row.error && <div className="card-health">{row.error}</div>}
            {asking === row.name ? (
              <ConfirmRow
                question={`Disconnect ${row.name}?`}
                detail={DISCONNECT_DETAIL}
                action="Disconnect"
                busy={busy !== null}
                onConfirm={() => void act(row.name, () => disconnectConnector(row.name))}
                onCancel={() => setAsking(null)}
              />
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
                {!remote && row.needsSignIn && row.enabled && (
                  <button
                    type="button"
                    disabled={busy !== null}
                    onClick={() => void act(row.name, () => signInConnector(row.name))}
                  >
                    {busy === row.name ? "Waiting for the browser…" : "Sign in"}
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

import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { codeSecretDelete, codeSecretList, codeSecretSet } from "./api";

export const SECRETS_TITLE = "Secrets";
export const SECRETS_NOTE =
  "Deploy runs get these as environment variables. Values stay in your system keychain and are hidden in output; only the names are saved here.";
export const SECRETS_NONE = "No secrets for this project yet.";
export const SECRETS_DELETE_NOTE = "Its value is removed from the keychain. A deploy that needs it stops until you add it again.";

/** Settings → Code → Secrets: names only. A value goes in once and never comes back. */
export function CodeSecrets() {
  const [names, setNames] = useState<string[] | null>(null);
  const [name, setName] = useState("");
  const [value, setValue] = useState("");
  const [deleting, setDeleting] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const valueField = useRef<HTMLInputElement>(null);

  useEffect(() => {
    codeSecretList().then(
      (list) => setNames(list.names),
      (err) => setNote(String(err)),
    );
  }, []);

  const typed = name.trim();
  const replacing = Boolean(names?.includes(typed));
  const canSave = !busy && Boolean(typed) && Boolean(value.trim());

  function onEnter(e: KeyboardEvent<HTMLInputElement>) {
    if (e.key === "Enter" && canSave) {
      e.preventDefault();
      void save();
    }
  }

  async function save() {
    setBusy(true);
    setNote(null);
    try {
      const list = await codeSecretSet(typed, value);
      setNames(list.names);
      // A new name shows in the list; a replaced value changes nothing visible, so say so.
      if (replacing) setNote(`Replaced ${typed}'s value.`);
      setName("");
    } catch (err) {
      setNote(String(err));
    } finally {
      // The value never stays on screen, saved or not.
      setValue("");
      setBusy(false);
    }
  }

  async function remove(target: string) {
    setBusy(true);
    setNote(null);
    try {
      const list = await codeSecretDelete(target, true);
      setNames(list.names);
    } catch (err) {
      setNote(String(err));
    } finally {
      setDeleting(null);
      setBusy(false);
    }
  }

  function replace(target: string) {
    setName(target);
    setValue("");
    setDeleting(null);
    valueField.current?.focus();
  }

  return (
    <div className="code-settings code-secrets" role="group" aria-label={SECRETS_TITLE}>
      <h3>{SECRETS_TITLE}</h3>
      <p className="note">{SECRETS_NOTE}</p>
      {names === null && !note && <p className="empty">Loading…</p>}
      {names !== null &&
        (names.length === 0 ? (
          <p className="empty">{SECRETS_NONE}</p>
        ) : (
          <ul className="rows code-secret-list" aria-label="Secret names">
            {names.map((n) => (
              <li key={n} className="code-secret-row">
                <code>{n}</code>
                <div className="card-buttons">
                  <button type="button" className="quiet" onClick={() => replace(n)} disabled={busy} aria-label={`Replace ${n}`}>
                    Replace
                  </button>
                  <button
                    type="button"
                    className="quiet danger"
                    onClick={() => setDeleting(n)}
                    disabled={busy}
                    aria-label={`Delete ${n}`}
                  >
                    Delete
                  </button>
                </div>
              </li>
            ))}
          </ul>
        ))}
      {deleting && (
        <div className="card confirm-window" role="group" aria-label={`Delete ${deleting}?`}>
          <p>
            Delete <code>{deleting}</code>?
          </p>
          <p className="note">{SECRETS_DELETE_NOTE}</p>
          <div className="card-buttons">
            <button type="button" className="quiet danger" onClick={() => void remove(deleting)} disabled={busy}>
              Delete
            </button>
            <button type="button" className="quiet" onClick={() => setDeleting(null)} disabled={busy}>
              Cancel
            </button>
          </div>
        </div>
      )}
      <div className="code-test-row">
        <label>
          Name
          <input
            value={name}
            spellCheck={false}
            autoComplete="off"
            placeholder="DEPLOY_TOKEN"
            disabled={busy}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={onEnter}
          />
        </label>
        <label>
          Value
          <input
            ref={valueField}
            type="password"
            value={value}
            spellCheck={false}
            autoComplete="off"
            disabled={busy}
            onChange={(e) => setValue(e.target.value)}
            onKeyDown={onEnter}
          />
        </label>
        <button type="button" onClick={() => void save()} disabled={!canSave}>
          {replacing ? "Replace" : "Add"}
        </button>
      </div>
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
    </div>
  );
}

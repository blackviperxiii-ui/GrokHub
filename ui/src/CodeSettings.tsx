import { useEffect, useState } from "react";
import { codeTestForget, codeTestRemembered, getCodeShip, saveCodeShip, type CodeShipSettings } from "./api";
import { Toggle } from "./Toggle";

export const CODE_SETTINGS_NOTE =
  "The first test run in a project asks and shows the exact command. After that, the same command runs without asking until you change it or turn this off.";
export const CODE_SETTINGS_NONE = "Nothing remembered yet. The next test run will ask you to confirm the command.";

export const SHIP_SETTINGS_RULES =
  "Always, in either mode: main and the repo's default branch are never pushed, nothing is force-pushed, and no branch is deleted without asking.";
export const SHIP_MODE_ASK = "Ask before every push and PR";
export const SHIP_MODE_AUTO = "Auto-beta: no ask for a branch made in the Code tab, on GitHub, into the PR base";

/** Settings → Code → Pushes and PRs. */
export function ShipSettings() {
  const [ship, setShip] = useState<CodeShipSettings | null>(null);
  const [base, setBase] = useState("");
  const [extra, setExtra] = useState("");
  const [note, setNote] = useState<string | null>(null);

  useEffect(() => {
    getCodeShip().then(
      (next) => {
        setShip(next);
        setBase(next.autoBase);
        setExtra(next.extraProtected.join(", "));
      },
      (err) => setNote(String(err)),
    );
  }, []);

  async function save(next: CodeShipSettings) {
    setNote(null);
    try {
      const saved = await saveCodeShip(next);
      setShip(saved);
      setBase(saved.autoBase);
      setExtra(saved.extraProtected.join(", "));
    } catch (err) {
      setNote(String(err));
    }
  }

  if (!ship) return note ? <p className="note">{note}</p> : null;
  const typedExtra = extra
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);
  const changed = base.trim() !== ship.autoBase || typedExtra.join(",") !== ship.extraProtected.join(",");
  return (
    <div className="code-settings" role="group" aria-label="Pushes and PRs">
      <h3>Pushes and PRs</h3>
      <fieldset className="channel">
        <legend>Mode</legend>
        <label>
          <input type="radio" name="code-ship-mode" checked={ship.mode === "ask"} onChange={() => void save({ ...ship, mode: "ask" })} />
          {SHIP_MODE_ASK}
        </label>
        <label>
          <input
            type="radio"
            name="code-ship-mode"
            checked={ship.mode === "autoBeta"}
            onChange={() => void save({ ...ship, mode: "autoBeta" })}
          />
          {SHIP_MODE_AUTO}
        </label>
      </fieldset>
      <p className="note">{SHIP_SETTINGS_RULES}</p>
      <div className="code-test-row">
        <label>
          PR base
          <input value={base} spellCheck={false} autoComplete="off" onChange={(e) => setBase(e.target.value)} />
        </label>
        <label>
          Also protected
          <input
            value={extra}
            spellCheck={false}
            autoComplete="off"
            placeholder="main, release"
            onChange={(e) => setExtra(e.target.value)}
          />
        </label>
        <button
          type="button"
          disabled={!changed}
          onClick={() => void save({ ...ship, autoBase: base.trim(), extraProtected: typedExtra })}
        >
          Save
        </button>
      </div>
      <Toggle checked={ship.hosts.other} onChange={(on) => void save({ ...ship, hosts: { ...ship.hosts, other: on } })}>
        Push to git hosts other than GitHub (always asks)
      </Toggle>
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
    </div>
  );
}

/** Settings → Code: forget the remembered test command for this project. */
export function CodeSettings() {
  const [command, setCommand] = useState<string | null | undefined>(undefined);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    codeTestRemembered().then((page) => setCommand(page.command), (err) => setNote(String(err)));
  }, []);

  async function forget() {
    setBusy(true);
    setNote(null);
    try {
      const page = await codeTestForget();
      setCommand(page.command);
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
    }
  }

  const remembered = Boolean(command);
  return (
    <div className="code-settings" role="group" aria-label="Code settings">
      <p className="note">{CODE_SETTINGS_NOTE}</p>
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
      {command === undefined && !note && <p className="empty">Loading…</p>}
      {command !== undefined && (
        <>
          <Toggle checked={remembered} disabled={busy || !remembered} onChange={(on) => { if (!on) void forget(); }}>
            Remember this project's test command
          </Toggle>
          {remembered ? (
            <p className="note">
              Remembered command: <code>{command}</code>
            </p>
          ) : (
            <p className="empty">{CODE_SETTINGS_NONE}</p>
          )}
        </>
      )}
      <ShipSettings />
    </div>
  );
}

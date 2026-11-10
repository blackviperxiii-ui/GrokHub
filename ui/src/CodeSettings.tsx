import { useEffect, useState } from "react";
import { codeTestForget, codeTestRemembered } from "./api";
import { Toggle } from "./Toggle";

export const CODE_SETTINGS_NOTE =
  "The first test run in a project asks and shows the exact command. After that, the same command runs without asking until you change it or turn this off.";
export const CODE_SETTINGS_NONE = "Nothing remembered yet. The next test run will ask you to confirm the command.";

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
    </div>
  );
}

import { useEffect, useState } from "react";
import {
  diagnosticsPreview,
  diagnosticsSave,
  getDiagnostics,
  listChats,
  setDiagnostics,
  type ChatRow,
  type DiagnosticsPage,
  type DiagnosticsPreview,
} from "./api";
import { StatusLine, useNote } from "./ui";
import { Toggle } from "./Toggle";

export const DIAGNOSTICS_NOTE =
  "Off by default. When on, you can make a redacted debug bundle: one zip with a JSON and a readable file, saved on this computer in your Diagnostics folder. Nothing leaves this machine unless you copy or share the file.";
export const NO_CHAT = "No chat text";
export const NO_CHAT_NOTE =
  "No chat text goes in unless you pick a chat. Keys, tokens, emails and your home folder name are hidden either way.";

/** Settings → Labs → Diagnostics mode: the cabin's switches (`app.json`), the
 * list of what a bundle holds, a preview of the redacted bundle, Save and
 * Copy summary. The redaction is the cabin's, done in the app. */
export function Diagnostics() {
  const [page, setPage] = useState<DiagnosticsPage | null>(null);
  const [chats, setChats] = useState<ChatRow[]>([]);
  const [chat, setChat] = useState("");
  const [shown, setShown] = useState<DiagnosticsPreview | null>(null);
  const [busy, setBusy] = useState(false);
  const { note, ok, fail } = useNote();

  useEffect(() => {
    getDiagnostics().then(setPage, fail);
    listChats().then(setChats, () => setChats([]));
  }, [fail]);

  async function change(mode: boolean, includeLogs: boolean) {
    try {
      setPage(await setDiagnostics(mode, includeLogs));
      setShown(null);
      ok(null);
    } catch (err) {
      fail(err);
    }
  }

  async function make() {
    setBusy(true);
    try {
      setShown(await diagnosticsPreview(chat || null));
      ok(null);
    } catch (err) {
      setShown(null);
      fail(err);
    } finally {
      setBusy(false);
    }
  }

  async function save() {
    setBusy(true);
    try {
      const saved = await diagnosticsSave();
      setShown(null);
      ok(saved.status);
    } catch (err) {
      fail(err);
    } finally {
      setBusy(false);
    }
  }

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      ok("Summary copied");
    } catch {
      fail("Can't copy here");
    }
  }

  if (!page) return <StatusLine note={note} className="note" />;

  return (
    <div className="labs" role="group" aria-label="Diagnostics">
      <h3>Diagnostics</h3>
      <Toggle checked={page.mode} onChange={(on) => void change(on, page.includeLogs)}>
        Diagnostics mode
      </Toggle>
      <p className="note">{DIAGNOSTICS_NOTE}</p>
      {page.mode && (
        <>
          <p className="note">Each Save includes:</p>
          <ul className="rows" aria-label="What a bundle includes">
            {page.preview.map((line) => (
              <li key={line}>{line}</li>
            ))}
          </ul>
          <Toggle checked={page.includeLogs} onChange={(on) => void change(page.mode, on)}>
            Include recent app logs
          </Toggle>
          <div className="labs-row">
            <label>
              Chat to include
              <select value={chat} onChange={(e) => setChat(e.target.value)}>
                <option value="">{NO_CHAT}</option>
                {chats.map((row) => (
                  <option key={row.id} value={row.id}>
                    {row.title || row.id}
                  </option>
                ))}
              </select>
            </label>
          </div>
          <p className="note">{NO_CHAT_NOTE}</p>
          <div className="card-buttons">
            <button type="button" disabled={busy} onClick={() => void make()}>
              Make a bundle
            </button>
          </div>
        </>
      )}
      {shown && (
        <div className="card" aria-label="Diagnostics preview">
          <div className="card-title">Diagnostics preview</div>
          <p className="note">{shown.summary}</p>
          <pre className="card-detail plain">{shown.markdown}</pre>
          <div className="card-buttons">
            <button type="button" disabled={busy} onClick={() => void save()}>
              Save
            </button>
            <button type="button" className="quiet" onClick={() => void copy(shown.summary)}>
              Copy summary
            </button>
            <button type="button" className="quiet" onClick={() => setShown(null)}>
              Cancel
            </button>
          </div>
          <p className="note">Save writes exactly this. Cancel writes nothing.</p>
        </div>
      )}
      <StatusLine note={note} className="note" />
    </div>
  );
}

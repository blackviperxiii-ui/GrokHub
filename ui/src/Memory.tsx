import { useEffect, useState, type FormEvent } from "react";
import {
  addMemory,
  approveMemoryRow,
  editMemoryRow,
  forgetMemoryRow,
  getMemory,
  rejectMemoryRow,
  saveMemory,
  searchMemory,
  setMemoryWrites,
  type MemoryHit,
  type MemoryNotes,
} from "./api";

type Scope = "workspace" | "global";
const SCOPES: Scope[] = ["workspace", "global"];

/** One editor: the text it started from (sent as the save's base) and the edit. */
type File = { base: string; draft: string };

export function Memory() {
  const [notes, setNotes] = useState<MemoryNotes | null>(null);
  const [files, setFiles] = useState<Record<Scope, File>>({
    workspace: { base: "", draft: "" },
    global: { base: "", draft: "" },
  });
  const [saving, setSaving] = useState(false);
  const [line, setLine] = useState("");
  const [lineGlobal, setLineGlobal] = useState(false);
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<MemoryHit[] | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [editId, setEditId] = useState<string | null>(null);
  const [draft, setDraft] = useState("");

  /** Shows what the files hold now. An editor with unsaved changes keeps
   * them (and its old base, so its save is checked) unless it is in `replace`. */
  function show(next: MemoryNotes, replace: Scope[]) {
    setNotes(next);
    setFiles((now) => {
      const out = { ...now };
      for (const scope of SCOPES) {
        if (replace.includes(scope) || now[scope].draft === now[scope].base) {
          out[scope] = { base: next[scope], draft: next[scope] };
        }
      }
      return out;
    });
  }

  useEffect(() => {
    getMemory().then(
      (next) => show(next, SCOPES),
      (err) => setNote(String(err)),
    );
  }, []);

  async function save(scope: Scope) {
    setSaving(true);
    try {
      const { notes: next, redacted } = await saveMemory(
        scope === "global",
        files[scope].draft,
        files[scope].base,
      );
      show(next, [scope]);
      setNote(redacted ? "Saved. A secret was replaced with [redacted]." : "Saved.");
    } catch (err) {
      setNote(String(err));
      // The file may have changed under the editor; show what it holds now.
      await getMemory().then(
        (next) => show(next, [scope]),
        () => {},
      );
    } finally {
      setSaving(false);
    }
  }

  async function add(event: FormEvent) {
    event.preventDefault();
    if (!line.trim()) return;
    try {
      const { status, notes: next } = await addMemory(line.trim(), lineGlobal);
      show(next, []);
      setLine("");
      setNote(`${status}.`);
    } catch (err) {
      setNote(String(err));
    }
  }

  async function apply(next: Promise<MemoryNotes>, done: string) {
    try {
      show(await next, []);
      setEditId(null);
      setNote(done);
    } catch (err) {
      setNote(String(err));
    }
  }

  async function search(event: FormEvent) {
    event.preventDefault();
    try {
      setHits(await searchMemory(query));
    } catch (err) {
      setNote(String(err));
    }
  }

  if (!notes) {
    return <section className="memory">{note ?? "Loading memory…"}</section>;
  }

  const editor = (scope: Scope, title: string, path: string) => (
    <div className="memory-file">
      <label>
        {title}
        <textarea
          rows={6}
          value={files[scope].draft}
          onChange={(e) => {
            const draft = e.target.value;
            setFiles((now) => ({ ...now, [scope]: { ...now[scope], draft } }));
          }}
        />
      </label>
      <div className="memory-file-foot">
        <code>{path}</code>
        <button
          type="button"
          disabled={saving || files[scope].draft === files[scope].base}
          onClick={() => void save(scope)}
        >
          Save {scope === "global" ? "global" : "project"} notes
        </button>
      </div>
    </div>
  );

  return (
    <section className="memory" aria-label="Memory">
      {notes.amr && (
        <p className="note">
          The cabin keeps memory in its AMR store, so chat recall does not read these files.
        </p>
      )}
      <label className="check">
        <input
          type="checkbox"
          aria-label="Remember new lines"
          checked={notes.writesOn}
          onChange={(e) =>
            void apply(
              setMemoryWrites(e.target.checked),
              e.target.checked
                ? "Memory is on."
                : "Memory is off. Nothing new is written.",
            )
          }
        />
        Remember new lines
      </label>
      {notes.rows.length > 0 && (
        <ul className="rows" aria-label="Learned lines">
          {notes.rows.map((row) => (
            <li key={row.id} className="card">
              {editId === row.id ? (
                <input
                  aria-label={`Edit text for ${row.text}`}
                  value={draft}
                  onChange={(e) => setDraft(e.target.value)}
                />
              ) : (
                <p>{row.text}</p>
              )}
              <p>{row.source}</p>
              <p>{row.updated}</p>
              {row.corrects && <p>Corrects: {row.corrects}</p>}
              {row.proposed && <p>Waiting for approval</p>}
              <div className="memory-row-actions">
                {editId === row.id ? (
                  <button
                    type="button"
                    disabled={!notes.writesOn || !draft.trim()}
                    onClick={() => void apply(editMemoryRow(row.id, draft.trim()), "Saved the line.")}
                  >
                    Save line
                  </button>
                ) : (
                  <button
                    type="button"
                    disabled={!notes.writesOn}
                    onClick={() => {
                      setEditId(row.id);
                      setDraft(row.text);
                    }}
                  >
                    Edit {row.text}
                  </button>
                )}
                <button
                  type="button"
                  onClick={() => void apply(forgetMemoryRow(row.id), "Forgot the line.")}
                >
                  Delete {row.text}
                </button>
                {row.proposed && (
                  <>
                    <button
                      type="button"
                      disabled={!notes.writesOn}
                      onClick={() => void apply(approveMemoryRow(row.id), "Approved the line.")}
                    >
                      Approve
                    </button>
                    <button
                      type="button"
                      onClick={() => void apply(rejectMemoryRow(row.id), "Rejected the line.")}
                    >
                      Reject
                    </button>
                  </>
                )}
              </div>
            </li>
          ))}
        </ul>
      )}
      <form className="add" onSubmit={add}>
        <input
          aria-label="New note"
          placeholder="Remember something"
          value={line}
          onChange={(e) => setLine(e.target.value)}
        />
        <label className="check">
          <input
            type="checkbox"
            checked={lineGlobal}
            onChange={(e) => setLineGlobal(e.target.checked)}
          />
          Global
        </label>
        <button type="submit" disabled={!line.trim()}>
          Remember
        </button>
      </form>
      {note && <p className="note">{note}</p>}
      {editor("workspace", "Project notes", notes.workspacePath)}
      {editor("global", "Global notes", notes.globalPath)}
      <form className="add" onSubmit={search}>
        <input
          aria-label="Search memory"
          placeholder="Search what chat would recall"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <button type="submit" disabled={!query.trim()}>
          Search
        </button>
      </form>
      {hits?.length === 0 && <p className="empty">Nothing matches.</p>}
      {hits && hits.length > 0 && (
        <ul className="rows" aria-label="Search results">
          {hits.map((hit, i) => (
            <li key={i} className="card hit">
              <span className="card-status">{hit.scope}</span>
              <pre className="card-detail">{hit.body}</pre>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

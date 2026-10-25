import { useEffect, useState, type FormEvent } from "react";
import {
  addMemory,
  getMemory,
  saveMemory,
  searchMemory,
  type MemoryHit,
  type MemoryNotes,
} from "./api";

type Scope = "workspace" | "global";

export function Memory() {
  const [notes, setNotes] = useState<MemoryNotes | null>(null);
  const [drafts, setDrafts] = useState<Record<Scope, string>>({ workspace: "", global: "" });
  const [line, setLine] = useState("");
  const [lineGlobal, setLineGlobal] = useState(false);
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<MemoryHit[] | null>(null);
  const [note, setNote] = useState<string | null>(null);

  function show(next: MemoryNotes) {
    setNotes(next);
    setDrafts({ workspace: next.workspace, global: next.global });
  }

  useEffect(() => {
    getMemory().then(show, (err) => setNote(String(err)));
  }, []);

  async function save(scope: Scope) {
    try {
      const { notes: next, redacted } = await saveMemory(
        scope === "global",
        drafts[scope],
        notes?.[scope] ?? "",
      );
      show(next);
      setNote(redacted ? "Saved. A secret was replaced with [redacted]." : "Saved.");
    } catch (err) {
      setNote(String(err));
      // The file may have changed under the editor; show what it holds now.
      getMemory().then(show, () => {});
    }
  }

  async function add(event: FormEvent) {
    event.preventDefault();
    if (!line.trim()) return;
    try {
      const { status, notes: next } = await addMemory(line.trim(), lineGlobal);
      show(next);
      setLine("");
      setNote(`${status}.`);
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
          value={drafts[scope]}
          onChange={(e) => setDrafts((now) => ({ ...now, [scope]: e.target.value }))}
        />
      </label>
      <div className="memory-file-foot">
        <code>{path}</code>
        <button
          type="button"
          disabled={drafts[scope] === notes[scope]}
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

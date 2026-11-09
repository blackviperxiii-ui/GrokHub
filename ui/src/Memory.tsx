import { useEffect, useState, type FormEvent } from "react";
import {
  PERSONAL_ADD_GROUPS,
  addMemory,
  addPersonalFact,
  approveMemoryRow,
  editMemoryRow,
  exportPersonal,
  forgetMemoryRow,
  getMemory,
  pauseMemoryRow,
  rejectMemoryRow,
  saveMemory,
  searchMemory,
  setMemoryWrites,
  wipePersonal,
  type MemoryHit,
  type MemoryNotes,
  type MemoryRow,
  type PersonalGroupKey,
} from "./api";
import { PRODUCT_NAME } from "./brand";
import { Toggle } from "./Toggle";
import { inShell } from "./transport";
import { ConfirmRow, StatusLine, useNote } from "./ui";

/** Page order of the groups; a group the page doesn't know goes last. */
export const PERSONAL_GROUP_ORDER = [
  "About you",
  "What you like",
  "Your routines",
  "What you need",
  "How you like things done",
  "Waiting for your OK",
];

/** Rows whose text, source or group holds every word of `query`. */
export function searchRows(rows: MemoryRow[], query: string): MemoryRow[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return rows;
  return rows.filter((row) => {
    const hay = `${row.text} ${row.source} ${row.group}`.toLowerCase();
    return words.every((word) => hay.includes(word));
  });
}

/** Rows by group, in page order, skipping empty groups. */
export function groupRows(rows: MemoryRow[]): [string, MemoryRow[]][] {
  const names = [...PERSONAL_GROUP_ORDER];
  for (const row of rows) if (!names.includes(row.group)) names.push(row.group);
  return names
    .map((name): [string, MemoryRow[]] => [name, rows.filter((row) => row.group === name)])
    .filter(([, list]) => list.length > 0);
}

/** "Paused, not used · you wrote it · 2026-10-07". */
export function rowMeta(row: MemoryRow): string {
  const bits: string[] = [];
  if (row.paused) bits.push("Paused, not used");
  else if (row.proposed) bits.push("Waiting for your OK");
  bits.push(row.source);
  const day = row.updated.slice(0, 10);
  if (day) bits.push(day);
  return bits.join(" · ");
}

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
  const { note, ok, fail } = useNote();
  const [editId, setEditId] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [find, setFind] = useState("");
  const [fact, setFact] = useState("");
  const [factGroup, setFactGroup] = useState<PersonalGroupKey>("about");
  const [wipeAsk, setWipeAsk] = useState(false);
  const [busy, setBusy] = useState(false);

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

  // Runs each time Memory shows; an editor with unsaved changes keeps them.
  useEffect(() => {
    getMemory().then(
      (next) => show(next, []),
      fail,
    );
  }, [fail]);

  async function save(scope: Scope) {
    setSaving(true);
    try {
      const { notes: next, redacted } = await saveMemory(
        scope === "global",
        files[scope].draft,
        files[scope].base,
      );
      show(next, [scope]);
      ok(redacted ? "Saved. A secret was replaced with [redacted]." : "Saved.");
    } catch (err) {
      fail(err);
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
      ok(`${status}.`);
    } catch (err) {
      fail(err);
    }
  }

  async function apply(next: Promise<MemoryNotes>, done: string) {
    try {
      show(await next, []);
      setEditId(null);
      ok(done);
    } catch (err) {
      fail(err);
    }
  }

  async function addFact(event: FormEvent) {
    event.preventDefault();
    if (!fact.trim()) return;
    try {
      show(await addPersonalFact(fact.trim(), factGroup), []);
      setFact("");
      ok("Added.");
    } catch (err) {
      fail(err);
    }
  }

  async function exportCopy() {
    try {
      ok(`Saved to ${await exportPersonal()}.`);
    } catch (err) {
      fail(err);
    }
  }

  async function wipe() {
    setBusy(true);
    try {
      const done = await wipePersonal();
      show(done.notes, SCOPES);
      ok(
        done.locked > 0
          ? `Wiped ${done.removed} lines. ${done.locked} private lines were locked and stay until you open them.`
          : `Wiped ${done.removed} lines. Personal is empty.`,
      );
    } catch (err) {
      fail(err);
    } finally {
      setBusy(false);
      setWipeAsk(false);
    }
  }

  async function search(event: FormEvent) {
    event.preventDefault();
    try {
      setHits(await searchMemory(query));
    } catch (err) {
      fail(err);
    }
  }

  if (!notes) {
    return <section className="memory">{note?.text ?? "Loading…"}</section>;
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

  const shown = searchRows(notes.rows, find);
  const row = (line: MemoryRow) => (
    <li key={line.id} className={line.paused ? "card paused" : "card"}>
      {editId === line.id ? (
        <input
          aria-label={`Edit text for ${line.text}`}
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
        />
      ) : (
        <p>{line.text}</p>
      )}
      <p className="card-status">{rowMeta(line)}</p>
      {line.corrects && <p>Corrects: {line.corrects}</p>}
      <div className="memory-row-actions">
        {line.proposed ? (
          <>
            <button
              type="button"
              aria-label={`OK ${line.text}`}
              disabled={!notes.writesOn}
              onClick={() => void apply(approveMemoryRow(line.id), "Saved.")}
            >
              OK
            </button>
            <button
              type="button"
              className="quiet"
              aria-label={`No ${line.text}`}
              onClick={() => void apply(rejectMemoryRow(line.id), "Dropped.")}
            >
              No
            </button>
          </>
        ) : editId === line.id ? (
          <>
            <button
              type="button"
              disabled={!notes.writesOn || !draft.trim()}
              onClick={() => void apply(editMemoryRow(line.id, draft.trim()), "Saved in your words.")}
            >
              Save
            </button>
            <button type="button" className="quiet" onClick={() => setEditId(null)}>
              Cancel
            </button>
          </>
        ) : (
          <>
            {!line.paused && (
              <button
                type="button"
                className="quiet"
                aria-label={`Edit ${line.text}`}
                disabled={!notes.writesOn}
                onClick={() => {
                  setEditId(line.id);
                  setDraft(line.text);
                }}
              >
                Edit
              </button>
            )}
            <button
              type="button"
              className="quiet"
              aria-label={`${line.paused ? "Use again" : "Don't use"} ${line.text}`}
              onClick={() =>
                void apply(
                  pauseMemoryRow(line.id, !line.paused),
                  line.paused ? "In use again." : "Kept, but not used until you say so.",
                )
              }
            >
              {line.paused ? "Use again" : "Don't use"}
            </button>
            <button
              type="button"
              className="quiet danger"
              aria-label={`Forget ${line.text}`}
              onClick={() => void apply(forgetMemoryRow(line.id), "Forgotten.")}
            >
              Forget
            </button>
          </>
        )}
      </div>
    </li>
  );

  return (
    <section className="memory" aria-label="Personal">
      <p className="note">
        What {PRODUCT_NAME} knows about you, and where it learned it. It all stays on this computer.
      </p>
      <Toggle
        checked={notes.writesOn}
        onChange={(on) =>
          void apply(
            setMemoryWrites(on),
            on ? "Learning from chats again." : "Not learning from chats. Nothing new is saved.",
          )
        }
      >
        Learn from chats
      </Toggle>
      <form className="add" onSubmit={addFact}>
        <select
          aria-label="Group"
          value={factGroup}
          onChange={(e) => setFactGroup(e.target.value as PersonalGroupKey)}
        >
          {PERSONAL_ADD_GROUPS.map(([key, name]) => (
            <option key={key} value={key}>
              {name}
            </option>
          ))}
        </select>
        <input
          aria-label="Add something about you"
          placeholder="In your words, like: I play after the kids are in bed"
          value={fact}
          onChange={(e) => setFact(e.target.value)}
        />
        <button type="submit" disabled={!fact.trim()}>
          Add
        </button>
      </form>
      <StatusLine note={note} className="note" />
      {notes.rows.length === 0 ? (
        <p className="empty">
          Nothing learned yet. {PRODUCT_NAME} adds to this as you chat, or add something above.
        </p>
      ) : (
        <>
          <input
            aria-label="Search what it knows"
            placeholder="Search"
            value={find}
            onChange={(e) => setFind(e.target.value)}
          />
          {shown.length === 0 && <p className="empty">None matched.</p>}
          {groupRows(shown).map(([group, list]) => (
            <div key={group} className="memory-group">
              <h3>{group}</h3>
              <ul className="rows" aria-label={group}>
                {list.map(row)}
              </ul>
            </div>
          ))}
        </>
      )}
      {inShell() && (
        <div className="memory-keep">
          <button type="button" className="quiet" onClick={() => void exportCopy()}>
            Export to Downloads
          </button>
          {wipeAsk ? (
            <ConfirmRow
              question="Wipe everything?"
              detail={`This deletes every line ${PRODUCT_NAME} learned about you and empties your global notes. Project notes stay. It can't be undone, so export first if you want a copy.`}
              action="Wipe for good"
              busy={busy}
              onConfirm={() => void wipe()}
              onCancel={() => setWipeAsk(false)}
            />
          ) : (
            <button type="button" className="quiet danger" onClick={() => setWipeAsk(true)}>
              Wipe everything
            </button>
          )}
        </div>
      )}
      <details className="memory-files">
        <summary>Notes files</summary>
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
        <Toggle checked={lineGlobal} onChange={setLineGlobal}>
          Global
        </Toggle>
        <button type="submit" disabled={!line.trim()}>
          Remember
        </button>
      </form>
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
      </details>
    </section>
  );
}

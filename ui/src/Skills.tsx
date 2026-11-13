import { useEffect, useState, type FormEvent } from "react";
import {
  createSkill,
  editSkill,
  getSkillDetail,
  getSkills,
  openSkillFolder,
  setSkillOn,
  type SkillDetail,
  type SkillRow,
  type SkillSource,
} from "./api";
import { Toggle } from "./Toggle";
import { StatusLine, useNote } from "./ui";

/** The groups, in page order. Built-in skills live in the Code tab, not here. */
export const SKILL_GROUPS: { source: SkillSource; label: string }[] = [
  { source: "cabin", label: "Cabin skills" },
  { source: "project", label: "Project skills" },
  { source: "user", label: "User skills" },
  { source: "plugin", label: "Plugin skills" },
];

export const SKILLS_EMPTY = "No skills found yet.";
export const SKILLS_NONE_MATCHED = "No skills match that search.";
export const SKILL_OFF_NOTE = "Off, chat won't use it.";

/** Rows whose name, description or trigger hold `query`, ignoring case. */
export function filterSkills(rows: SkillRow[], query: string): SkillRow[] {
  const q = query.trim().toLowerCase();
  if (!q) return rows;
  return rows.filter((row) =>
    [row.name, row.description, row.trigger].some((text) =>
      text.toLowerCase().includes(q),
    ),
  );
}

/** One form field: a title, a quiet hint, and the words themselves. */
function Field({
  title,
  hint,
  value,
  onChange,
  lines,
}: {
  title: string;
  hint?: string;
  value: string;
  onChange: (value: string) => void;
  lines: number;
}) {
  return (
    <label className="skill-field">
      <span className="skill-field-title">{title}</span>
      {hint && <span className="skill-field-hint">{hint}</span>}
      <textarea
        aria-label={title}
        rows={lines}
        value={value}
        onChange={(event) => onChange(event.target.value)}
      />
    </label>
  );
}

/** The skills chat can use. Use puts a skill's trigger in the chat box. */
export function Skills({
  onUse,
  onOpenConnectors,
  remote = false,
}: {
  onUse: (text: string) => void;
  onOpenConnectors: () => void;
  /** The web app lists skills but can't open this computer's folders. */
  remote?: boolean;
}) {
  const [rows, setRows] = useState<SkillRow[] | null>(null);
  const { note, fail, clear } = useNote();
  const [query, setQuery] = useState("");
  const [busy, setBusy] = useState(false);
  const [making, setMaking] = useState(false);
  const [newName, setNewName] = useState("");
  const [newWhat, setNewWhat] = useState("");
  const [edit, setEdit] = useState<SkillDetail | null>(null);

  async function refresh() {
    setBusy(true);
    try {
      setRows((await getSkills()).rows);
      clear();
    } catch (err) {
      fail(err);
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    getSkills().then((page) => setRows(page.rows), fail);
  }, [fail]);

  async function openFolder(row: SkillRow) {
    try {
      await openSkillFolder(row.source, row.name);
      clear();
    } catch (err) {
      fail(err);
    }
  }

  async function switchSkill(row: SkillRow, on: boolean) {
    try {
      setRows((await setSkillOn(row.name, on)).rows);
      clear();
    } catch (err) {
      fail(err);
    }
  }

  async function saveNew(event: FormEvent) {
    event.preventDefault();
    try {
      setRows((await createSkill(newName, newWhat)).rows);
      setMaking(false);
      setNewName("");
      setNewWhat("");
      clear();
    } catch (err) {
      fail(err);
    }
  }

  async function startEdit(row: SkillRow) {
    try {
      setEdit(await getSkillDetail(row.name));
      clear();
    } catch (err) {
      fail(err);
    }
  }

  async function saveEdit(event: FormEvent) {
    event.preventDefault();
    if (!edit) return;
    try {
      setRows(
        (await editSkill(edit.name, edit.description, edit.trigger, edit.instructions))
          .rows,
      );
      setEdit(null);
      clear();
    } catch (err) {
      fail(err);
    }
  }

  const shown = rows ? filterSkills(rows, query) : [];
  return (
    <section className="skills" aria-label="Skills">
      <div className="card-buttons">
        <input
          type="search"
          aria-label="Search skills"
          placeholder="Search by name, description or trigger"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
        />
        <button type="button" disabled={busy} onClick={() => void refresh()}>
          {busy ? "Refreshing…" : "Refresh"}
        </button>
        {!remote && !making && (
          <button type="button" onClick={() => setMaking(true)}>
            New skill
          </button>
        )}
        <button type="button" className="quiet" onClick={onOpenConnectors}>
          Connectors
        </button>
      </div>
      <p className="note">
        Skills chat can use: this computer's cabin skills, the project's and
        your own, and the ones enabled plugins bring. Switch one off to keep it
        but stop chat using it. Make a new one with New skill, or Edit one of
        this computer's skills to change it.
      </p>
      {making && (
        <form
          className="skill-edit"
          aria-label="New skill"
          onSubmit={(event) => void saveNew(event)}
        >
          <label className="skill-field">
            <span className="skill-field-title">Name</span>
            <span className="skill-field-hint">
              A few words, like Clip trimmer.
            </span>
            <input
              type="text"
              aria-label="Name"
              value={newName}
              onChange={(event) => setNewName(event.target.value)}
            />
          </label>
          <Field
            title="What it should do"
            hint="In your own words. You can add steps after with Edit."
            value={newWhat}
            onChange={setNewWhat}
            lines={3}
          />
          <div className="card-buttons">
            <button type="submit">Save skill</button>
            <button
              type="button"
              className="quiet"
              onClick={() => setMaking(false)}
            >
              Cancel
            </button>
          </div>
        </form>
      )}
      <StatusLine note={note} />
      {rows === null && !note && <p className="empty">Loading skills…</p>}
      {rows?.length === 0 && <p className="empty">{SKILLS_EMPTY}</p>}
      {rows && rows.length > 0 && shown.length === 0 && (
        <p className="empty">{SKILLS_NONE_MATCHED}</p>
      )}
      {SKILL_GROUPS.map(({ source, label }) => {
        const group = shown.filter((row) => row.source === source);
        if (group.length === 0) return null;
        return (
          <div key={source} className="skill-group">
            <h3>{label}</h3>
            <ul className="rows" aria-label={label}>
              {group.map((row) => (
                <li
                  key={`${row.source}:${row.name}`}
                  className="card skill"
                  aria-label={row.name}
                >
                  <div className="card-title">{row.name}</div>
                  {row.description && (
                    <div className="card-status">{row.description}</div>
                  )}
                  <div className="card-detail-line">{row.trigger}</div>
                  {!row.on && (
                    <div className="card-status">{SKILL_OFF_NOTE}</div>
                  )}
                  {!remote && (
                    <Toggle
                      checked={row.on}
                      onChange={(on) => void switchSkill(row, on)}
                    >
                      Chat can use it
                    </Toggle>
                  )}
                  <div className="card-buttons">
                    {row.on && (
                      <button type="button" onClick={() => onUse(row.trigger)}>
                        Use
                      </button>
                    )}
                    {!remote && row.source === "cabin" && (
                      <button
                        type="button"
                        className="quiet"
                        onClick={() => void startEdit(row)}
                      >
                        Edit
                      </button>
                    )}
                    {!remote && row.folder && (
                      <button
                        type="button"
                        className="quiet"
                        title={row.folder}
                        onClick={() => void openFolder(row)}
                      >
                        Open folder
                      </button>
                    )}
                  </div>
                  {edit?.name === row.name && (
                    <form
                      className="skill-edit"
                      aria-label={`Edit ${row.name}`}
                      onSubmit={(event) => void saveEdit(event)}
                    >
                      <Field
                        title="What it does"
                        value={edit.description}
                        onChange={(value) =>
                          setEdit({ ...edit, description: value })
                        }
                        lines={2}
                      />
                      <Field
                        title="When to use it"
                        hint="The kind of ask that should pick it."
                        value={edit.trigger}
                        onChange={(value) =>
                          setEdit({ ...edit, trigger: value })
                        }
                        lines={2}
                      />
                      <Field
                        title="Steps"
                        hint="What to do, one step a line."
                        value={edit.instructions}
                        onChange={(value) =>
                          setEdit({ ...edit, instructions: value })
                        }
                        lines={6}
                      />
                      <div className="card-buttons">
                        <button type="submit">Save changes</button>
                        <button
                          type="button"
                          className="quiet"
                          onClick={() => setEdit(null)}
                        >
                          Cancel
                        </button>
                      </div>
                    </form>
                  )}
                </li>
              ))}
            </ul>
          </div>
        );
      })}
    </section>
  );
}

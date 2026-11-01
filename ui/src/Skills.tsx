import { useEffect, useState } from "react";
import { getSkills, openSkillFolder, type SkillRow, type SkillSource } from "./api";
import { PRODUCT_NAME } from "./brand";
import { StatusLine, useNote } from "./ui";

/** The groups, in page order. */
export const SKILL_GROUPS: { source: SkillSource; label: string }[] = [
  { source: "cabin", label: "Cabin skills" },
  { source: "project", label: "Project skills" },
  { source: "user", label: "User skills" },
  { source: "plugin", label: "Plugin skills" },
  { source: "builtin", label: "Built-in skills" },
];

export const SKILLS_EMPTY = "No skills found yet.";
export const SKILLS_NONE_MATCHED = "No skills match that search.";

/** Rows whose name, description or trigger hold `query`, ignoring case. */
export function filterSkills(rows: SkillRow[], query: string): SkillRow[] {
  const q = query.trim().toLowerCase();
  if (!q) return rows;
  return rows.filter((row) =>
    [row.name, row.description, row.trigger].some((text) => text.toLowerCase().includes(q)),
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
    getSkills().then(
      (page) => setRows(page.rows),
      fail,
    );
  }, [fail]);

  async function openFolder(row: SkillRow) {
    try {
      await openSkillFolder(row.source, row.name);
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
        <button type="button" className="quiet" onClick={onOpenConnectors}>
          Connectors
        </button>
      </div>
      <p className="note">
        Skills chat can use: this computer's cabin skills, the project's and your own, and the
        ones enabled plugins bring. Create or edit them in the {PRODUCT_NAME} cabin.
      </p>
      <StatusLine note={note} />
      {rows === null && !note && <p className="empty">Loading skills…</p>}
      {rows?.length === 0 && <p className="empty">{SKILLS_EMPTY}</p>}
      {rows && rows.length > 0 && shown.length === 0 && <p className="empty">{SKILLS_NONE_MATCHED}</p>}
      {SKILL_GROUPS.map(({ source, label }) => {
        const group = shown.filter((row) => row.source === source);
        if (group.length === 0) return null;
        return (
          <div key={source} className="skill-group">
            <h3>{label}</h3>
            <ul className="rows" aria-label={label}>
              {group.map((row) => (
                <li key={`${row.source}:${row.name}`} className="card skill" aria-label={row.name}>
                  <div className="card-title">{row.name}</div>
                  {row.description && <div className="card-status">{row.description}</div>}
                  <div className="card-detail-line">{row.trigger}</div>
                  <div className="card-buttons">
                    <button type="button" onClick={() => onUse(row.trigger)}>
                      Use
                    </button>
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
                </li>
              ))}
            </ul>
          </div>
        );
      })}
    </section>
  );
}

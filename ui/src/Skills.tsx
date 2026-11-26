import { useEffect, useState, type FormEvent } from "react";
import {
  createSkill,
  editSkill,
  getSkillDetail,
  getSkillLibrary,
  getSkills,
  installLibraryPack,
  installLibrarySkill,
  installSkillsHub,
  openSkillFolder,
  searchSkillsHub,
  setSkillOn,
  type SkillDetail,
  type SkillLibraryItem,
  type SkillLibraryPage,
  type SkillRow,
  type SkillSource,
  type SkillsHubHit,
} from "./api";
import { Toggle } from "./Toggle";
import { PageHead, StatusLine, useNote } from "./ui";

export const SKILLS_HUB_NOTE =
  "Search ClawHub, SkillsSH, GitHub (owner/repo), or a host with /.well-known/skills. New installs go through the skill scanner and quarantine.";

export const SKILLS_LIBRARY_NOTE =
  "Browse curated workflows and install the Everyday Life Pack in one click. Bundled and pack skills run through the same safety check as self-authored skills.";

/** The groups, in page order. Built-in skills live in the Code tab, not here. */
export const SKILL_GROUPS: { source: SkillSource; label: string }[] = [
  { source: "cabin", label: "App skills" },
  { source: "project", label: "Project skills" },
  { source: "user", label: "User skills" },
  { source: "plugin", label: "Plugin skills" },
];

export const SKILLS_EMPTY = "No skills found yet.";
export const SKILLS_NONE_MATCHED = "No skills match that search.";
export const SKILL_OFF_NOTE = "Off, chat won't use it.";

/** Rows whose name, description, trigger, category or tags hold `query`, ignoring case. */
export function filterSkills(rows: SkillRow[], query: string): SkillRow[] {
  const q = query.trim().toLowerCase();
  if (!q) return rows;
  return rows.filter((row) =>
    [
      row.name,
      row.description,
      row.trigger,
      row.category ?? "",
      ...(row.tags ?? []),
    ].some((text) => text.toLowerCase().includes(q)),
  );
}

/** Filter curated Skills Library items by category and free-text search. */
export function filterLibrarySkills(
  items: SkillLibraryItem[],
  category: string,
  query: string,
): SkillLibraryItem[] {
  const q = query.trim().toLowerCase();
  return items.filter((item) => {
    if (category && item.category !== category) return false;
    if (!q) return true;
    return [
      item.name,
      item.description,
      item.category,
      item.trigger,
      ...item.tags,
    ].some((text) => text.toLowerCase().includes(q));
  });
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
  const { note, fail, clear, ok } = useNote();
  const [query, setQuery] = useState("");
  const [busy, setBusy] = useState(false);
  const [making, setMaking] = useState(false);
  const [newName, setNewName] = useState("");
  const [newWhat, setNewWhat] = useState("");
  const [edit, setEdit] = useState<SkillDetail | null>(null);
  const [hubQuery, setHubQuery] = useState("");
  const [hubHits, setHubHits] = useState<SkillsHubHit[] | null>(null);
  const [hubBusy, setHubBusy] = useState(false);
  const [library, setLibrary] = useState<SkillLibraryPage | null>(null);
  const [libCategory, setLibCategory] = useState("");
  const [libQuery, setLibQuery] = useState("");
  const [previewId, setPreviewId] = useState<string | null>(null);
  const [libBusy, setLibBusy] = useState(false);

  async function loadLibrary() {
    if (remote || typeof getSkillLibrary !== "function") return;
    try {
      const page = await getSkillLibrary();
      if (page) setLibrary(page);
    } catch {
      // Optional browse surface: keep installed skills working if library fails.
    }
  }

  async function refresh() {
    setBusy(true);
    try {
      setRows((await getSkills()).rows);
      await loadLibrary();
      clear();
    } catch (err) {
      fail(err);
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    getSkills().then((page) => setRows(page.rows), fail);
    if (!remote && typeof getSkillLibrary === "function") {
      void getSkillLibrary()
        ?.then((page) => {
          if (page) setLibrary(page);
        })
        .catch(() => {});
    }
  }, [fail, remote]);

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
      await loadLibrary();
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

  async function runHubSearch(event: FormEvent) {
    event.preventDefault();
    if (!hubQuery.trim() || hubBusy) return;
    setHubBusy(true);
    try {
      setHubHits(await searchSkillsHub(hubQuery));
      clear();
    } catch (err) {
      setHubHits([]);
      fail(err);
    } finally {
      setHubBusy(false);
    }
  }

  async function installHubHit(hit: SkillsHubHit) {
    if (hubBusy) return;
    setHubBusy(true);
    try {
      const done = await installSkillsHub(hit);
      setRows(done.page.rows);
      setHubHits((prev) =>
        prev ? prev.filter((row) => !(row.source === hit.source && row.id === hit.id)) : prev,
      );
      ok(done.note);
    } catch (err) {
      fail(err);
    } finally {
      setHubBusy(false);
    }
  }

  async function installOneFromLibrary(item: SkillLibraryItem) {
    if (libBusy) return;
    setLibBusy(true);
    try {
      const done = await installLibrarySkill(item.id);
      setRows(done.page.rows);
      setLibrary(done.library);
      ok(done.note);
    } catch (err) {
      fail(err);
    } finally {
      setLibBusy(false);
    }
  }

  async function installPackFromLibrary(packId: string) {
    if (libBusy) return;
    setLibBusy(true);
    try {
      const done = await installLibraryPack(packId);
      setRows(done.page.rows);
      setLibrary(done.library);
      ok(done.note);
    } catch (err) {
      fail(err);
    } finally {
      setLibBusy(false);
    }
  }

  const shown = rows ? filterSkills(rows, query) : [];
  const libraryItems = library
    ? filterLibrarySkills(library.items, libCategory, libQuery)
    : [];

  return (
    <section className="skills" aria-label="Skills">
      <PageHead title="Skills" />
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
        Skills chat can use: this app's own, the project's and yours, and the
        ones enabled plugins bring. Switch one off to keep it but stop chat
        using it. Make a new one with New skill, or Edit one of this app's own
        to change it.
      </p>
      {!remote && library && library.items.length > 0 && (
        <section className="skill-group" aria-label="Skills Library">
          <h3>Skills Library</h3>
          <p className="note">{SKILLS_LIBRARY_NOTE}</p>
          {library.packs.map((pack) => {
            const allInstalled = pack.total > 0 && pack.installed >= pack.total;
            return (
              <div
                key={pack.id}
                className="card skill skill-pack-card"
                aria-label={pack.name}
              >
                <div className="skill-pack-head">
                  <div>
                    <div className="card-title">{pack.name}</div>
                    <div className="card-status">
                      {pack.description} ({pack.installed} of {pack.total} installed)
                    </div>
                  </div>
                  <div className="card-buttons">
                    <button
                      type="button"
                      disabled={libBusy || allInstalled}
                      onClick={() => void installPackFromLibrary(pack.id)}
                    >
                      {allInstalled ? "Pack installed" : "Install pack"}
                    </button>
                  </div>
                </div>
              </div>
            );
          })}
          <div className="skill-library-controls">
            <input
              type="search"
              aria-label="Search Skills Library"
              placeholder="Browse Skills Library by topic or keyword"
              value={libQuery}
              onChange={(event) => setLibQuery(event.target.value)}
            />
            <div className="skill-library-filters" role="group" aria-label="Library categories">
              <button
                type="button"
                className={libCategory === "" ? "" : "quiet"}
                onClick={() => setLibCategory("")}
              >
                All
              </button>
              {library.categories.map((category) => (
                <button
                  key={category}
                  type="button"
                  className={libCategory === category ? "" : "quiet"}
                  onClick={() => setLibCategory(category)}
                >
                  {category}
                </button>
              ))}
            </div>
          </div>
          {libraryItems.length === 0 ? (
            <p className="empty">No Skills Library items matched that filter.</p>
          ) : (
            <div className="rows" aria-label="Skills Library items">
              {libraryItems.map((item) => {
                const open = previewId === item.id;
                return (
                  <article
                    key={item.id}
                    className="card skill"
                    aria-label={item.name}
                  >
                    <div className="skill-row-head">
                      <span className="card-title">{item.name}</span>
                      <span className="skill-category-tag">{item.category}</span>
                    </div>
                    <div className="card-status">{item.description}</div>
                    <div className="card-detail-line">{item.trigger}</div>
                    <div className="card-buttons">
                      {item.installed ? (
                        <button type="button" onClick={() => onUse(item.trigger)}>
                          Use
                        </button>
                      ) : (
                        <button
                          type="button"
                          disabled={libBusy}
                          onClick={() => void installOneFromLibrary(item)}
                        >
                          Install
                        </button>
                      )}
                      <button
                        type="button"
                        className="quiet"
                        onClick={() => setPreviewId(open ? null : item.id)}
                      >
                        {open ? "Hide preview" : "Preview"}
                      </button>
                    </div>
                    {open && (
                      <div className="skill-preview">
                        <div className="skill-field-title">Steps</div>
                        <pre>{item.instructions}</pre>
                        {item.pitfalls && (
                          <>
                            <div className="skill-field-title">Safety & guardrails</div>
                            <div className="card-status">{item.pitfalls}</div>
                          </>
                        )}
                      </div>
                    )}
                  </article>
                );
              })}
            </div>
          )}
        </section>
      )}
      {!remote && (
        <div className="skill-group" aria-label="Skills Hub">
          <h3>Skills Hub</h3>
          <p className="note">{SKILLS_HUB_NOTE}</p>
          <form className="card-buttons" onSubmit={(event) => void runHubSearch(event)}>
            <input
              type="search"
              aria-label="Search Skills Hub"
              placeholder="Search registries, owner/repo, or example.com"
              value={hubQuery}
              onChange={(event) => setHubQuery(event.target.value)}
            />
            <button type="submit" disabled={hubBusy || !hubQuery.trim()}>
              {hubBusy ? "Searching…" : "Search Hub"}
            </button>
          </form>
          {hubHits && hubHits.length === 0 && (
            <p className="empty">No hub skills matched that search.</p>
          )}
          {hubHits && hubHits.length > 0 && (
            <ul className="rows" aria-label="Skills Hub results">
              {hubHits.map((hit) => (
                <li
                  key={`${hit.source}:${hit.id}`}
                  className="card skill"
                  aria-label={hit.name}
                >
                  <div className="card-title">{hit.name}</div>
                  <div className="card-status">
                    {hit.source}
                    {hit.description ? ` · ${hit.description}` : ""}
                  </div>
                  <div className="card-buttons">
                    <button
                      type="button"
                      disabled={hubBusy}
                      onClick={() => void installHubHit(hit)}
                    >
                      Install
                    </button>
                  </div>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
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
                  <div className="skill-row-head">
                    <div className="card-title">{row.name}</div>
                    {row.category && (
                      <span className="skill-category-tag">{row.category}</span>
                    )}
                  </div>
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

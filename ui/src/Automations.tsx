import { useEffect, useState, type FormEvent } from "react";
import {
  addAutomation,
  addSuggestedAutomation,
  editAutomation,
  fillBlueprintSeed,
  listAutomationBlueprints,
  listAutomations,
  listSuggestedAutomations,
  onAutomationRan,
  removeAutomation,
  runAutomation,
  setAutomationEnabled,
  type AutomationRow,
  type Blueprint,
  type RunResult,
  type SuggestedTile,
} from "./api";
import { PRODUCT_NAME } from "./brand";
import { ConfirmRow } from "./ui";

export function resultLine(result: RunResult): string {
  if (result.error) return result.error;
  return result.text.trim() || "Finished with no reply.";
}

export const SUGGESTED_NOTE = "Suggested from your recent work";

export const BLUEPRINTS_NOTE =
  "Starters with a few fields to fill — script-first checks that fail closed before a model call.";

export const AUTOMATIONS_EMPTY =
  "No automations yet. Describe one above in plain words and it runs on its own schedule.";

/** A finished run, for the notice in chat and the Activity Dock. */
export type RunNotice = { id: string; name: string; ok: boolean; text: string };

/** `active`: the tab is showing. The page stays mounted so a Run now in
 * flight keeps its button and its reply; it reloads the list on each visit.
 * `onStarted` and `onFinished` hear about each run, so chat and the
 * Activity Dock can say so. */
export function Automations({
  active = true,
  onStarted,
  onFinished,
}: {
  active?: boolean;
  onStarted?: (id: string, name: string) => void;
  onFinished?: (notice: RunNotice) => void;
}) {
  const [rows, setRows] = useState<AutomationRow[] | null>(null);
  const [draft, setDraft] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const [running, setRunning] = useState<string | null>(null);
  const [results, setResults] = useState<Record<string, string>>({});
  const [editing, setEditing] = useState<{ id: string; text: string } | null>(null);
  const [asking, setAsking] = useState<string | null>(null);
  const [tiles, setTiles] = useState<SuggestedTile[]>([]);
  const [blueprints, setBlueprints] = useState<Blueprint[]>([]);
  const [adding, setAdding] = useState<string | null>(null);
  const [filling, setFilling] = useState<string | null>(null);
  const [slotValues, setSlotValues] = useState<Record<string, string>>({});

  useEffect(() => {
    if (active) listAutomations().then(setRows, (err) => setNote(String(err)));
  }, [active]);

  // The tiles hide jobs already running, so they follow every change to the
  // list. They are extras: a page that can't list them shows none.
  useEffect(() => {
    if (!active || rows === null) return;
    let stale = false;
    listSuggestedAutomations().then(
      (got) => !stale && setTiles(got),
      () => !stale && setTiles([]),
    );
    return () => {
      stale = true;
    };
  }, [active, rows]);

  useEffect(() => {
    if (!active) return;
    let stale = false;
    listAutomationBlueprints().then(
      (got) => !stale && setBlueprints(got),
      () => !stale && setBlueprints([]),
    );
    return () => {
      stale = true;
    };
  }, [active]);

  // A scheduled run the shell fired: show its reply and the row's new count.
  useEffect(() => {
    if (!active) return;
    let stop: (() => void) | undefined;
    let dropped = false;
    void onAutomationRan(({ id, result }) => {
      setResults((now) => ({ ...now, [id]: resultLine(result) }));
      listAutomations().then(setRows, (err) => setNote(String(err)));
    }).then((unlisten) => {
      if (dropped) unlisten();
      else stop = unlisten;
    });
    return () => {
      dropped = true;
      stop?.();
    };
  }, [active]);

  async function saveEdit(event: FormEvent) {
    event.preventDefault();
    if (!editing) return;
    try {
      setRows(await editAutomation(editing.id, editing.text.trim()));
      setEditing(null);
      setNote(null);
    } catch (err) {
      setNote(String(err));
    }
  }

  async function act(step: () => Promise<AutomationRow[]>) {
    try {
      setRows(await step());
      setNote(null);
    } catch (err) {
      setNote(String(err));
    }
  }

  async function add(event: FormEvent) {
    event.preventDefault();
    const text = draft.trim();
    if (!text) return;
    try {
      setRows(await addAutomation(text));
      setDraft("");
      setNote(null);
    } catch (err) {
      setNote(String(err));
    }
  }

  async function addTile(seed: string) {
    setAdding(seed);
    try {
      setRows(await addSuggestedAutomation(seed));
      setNote(null);
    } catch (err) {
      setNote(String(err));
    } finally {
      setAdding(null);
    }
  }

  function openBlueprint(tile: Blueprint) {
    const next: Record<string, string> = {};
    for (const slot of tile.slots) next[slot.key] = slot.placeholder;
    setSlotValues(next);
    setFilling(tile.id);
    setNote(null);
  }

  async function saveBlueprint(tile: Blueprint) {
    setAdding(tile.id);
    try {
      const values: Record<string, string> = {};
      for (const slot of tile.slots) values[slot.key] = slotValues[slot.key] ?? "";
      const seed = fillBlueprintSeed(tile.seed, values);
      setRows(await addAutomation(seed));
      setFilling(null);
      setSlotValues({});
      setNote(null);
    } catch (err) {
      setNote(String(err));
    } finally {
      setAdding(null);
    }
  }

  async function run(id: string) {
    const name = rows?.find((row) => row.id === id)?.name ?? "Automation";
    setRunning(id);
    onStarted?.(id, name);
    try {
      const result = await runAutomation(id);
      setResults((now) => ({ ...now, [id]: resultLine(result) }));
      onFinished?.({ id, name, ok: !result.error, text: resultLine(result) });
      setRows(await listAutomations());
    } catch (err) {
      setResults((now) => ({ ...now, [id]: String(err) }));
      onFinished?.({ id, name, ok: false, text: String(err) });
    } finally {
      setRunning(null);
    }
  }

  return (
    <section className="automations" aria-label="Automations">
      <form className="add" onSubmit={add}>
        <input
          aria-label="New automation"
          placeholder="every weekday at 9am summarize my inbox"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
        />
        <button type="submit" disabled={!draft.trim()}>
          Add
        </button>
      </form>
      <p className="note">
        {PRODUCT_NAME} runs these on schedule while it is open or in the tray. A run follows your
        Approvals setting, and anything that would need your OK is skipped.
      </p>
      {note && (
        <p className="error" role="alert">
          {note}
        </p>
      )}
      {rows === null && !note && <p className="empty">Loading automations…</p>}
      {rows?.length === 0 && <p className="empty">{AUTOMATIONS_EMPTY}</p>}
      <ul className="rows">
        {rows?.map((row) => (
          <li key={row.id} className="card automation" aria-label={row.name}>
            <div className="card-title">{row.name}</div>
            <div className="card-status">{row.summary}</div>
            {row.health && <div className="card-health">{row.health}</div>}
            {results[row.id] && <pre className="card-detail">{results[row.id]}</pre>}
            {editing?.id === row.id && (
              <form className="add" onSubmit={saveEdit}>
                <input
                  aria-label={`Edit ${row.name}`}
                  value={editing.text}
                  onChange={(e) => setEditing({ id: row.id, text: e.target.value })}
                />
                <button type="submit" disabled={!editing.text.trim()}>
                  Save
                </button>
                <button type="button" className="quiet" onClick={() => setEditing(null)}>
                  Cancel
                </button>
              </form>
            )}
            {asking === row.id ? (
              <ConfirmRow
                question={`Remove ${row.name}?`}
                detail="It won't run again."
                action="Remove"
                onConfirm={() => {
                  setAsking(null);
                  void act(() => removeAutomation(row.id));
                }}
                onCancel={() => setAsking(null)}
              />
            ) : (
            <div className="card-buttons">
              <button
                type="button"
                disabled={running !== null}
                onClick={() => void run(row.id)}
              >
                {running === row.id ? "Running…" : "Run now"}
              </button>
              <button
                type="button"
                className="quiet"
                onClick={() => void act(() => setAutomationEnabled(row.id, !row.enabled))}
              >
                {row.enabled ? "Pause" : "Resume"}
              </button>
              {row.describe !== null && (
                <button
                  type="button"
                  className="quiet"
                  disabled={editing?.id === row.id}
                  onClick={() => setEditing({ id: row.id, text: row.describe ?? "" })}
                >
                  Edit
                </button>
              )}
              <button
                type="button"
                className="quiet danger"
                onClick={() => setAsking(row.id)}
              >
                Remove…
              </button>
            </div>
            )}
          </li>
        ))}
      </ul>
      {blueprints.length > 0 && (
        <section className="blueprints" aria-label="Blueprints">
          <h3>Blueprints</h3>
          <p className="note">{BLUEPRINTS_NOTE}</p>
          <ul className="rows">
            {blueprints.map((tile) => (
              <li key={tile.id} className="card automation blueprint" aria-label={tile.title}>
                <div className="card-title">{tile.title}</div>
                <div className="card-status">{tile.body}</div>
                {filling === tile.id ? (
                  <form
                    className="blueprint-fill"
                    onSubmit={(event) => {
                      event.preventDefault();
                      void saveBlueprint(tile);
                    }}
                  >
                    {tile.slots.map((slot) => (
                      <label key={slot.key} className="blueprint-slot">
                        <span>{slot.label}</span>
                        <input
                          aria-label={`${tile.title} ${slot.label}`}
                          value={slotValues[slot.key] ?? ""}
                          placeholder={slot.placeholder}
                          onChange={(e) =>
                            setSlotValues((now) => ({ ...now, [slot.key]: e.target.value }))
                          }
                        />
                      </label>
                    ))}
                    <div className="card-buttons">
                      <button type="submit" disabled={adding !== null}>
                        {adding === tile.id ? "Saving…" : "Save"}
                      </button>
                      <button
                        type="button"
                        className="quiet"
                        disabled={adding !== null}
                        onClick={() => {
                          setFilling(null);
                          setSlotValues({});
                        }}
                      >
                        Cancel
                      </button>
                    </div>
                  </form>
                ) : (
                  <div className="card-buttons">
                    <button
                      type="button"
                      disabled={adding !== null}
                      onClick={() => openBlueprint(tile)}
                    >
                      Use
                    </button>
                  </div>
                )}
              </li>
            ))}
          </ul>
        </section>
      )}
      {tiles.length > 0 && (
        <section className="suggested" aria-label="Suggested">
          <h3>Suggested</h3>
          <p className="note">{SUGGESTED_NOTE}</p>
          <ul className="rows">
            {tiles.map((tile) => (
              <li key={tile.seed} className="card automation" aria-label={tile.title}>
                <div className="card-title">{tile.title}</div>
                <div className="card-status">{tile.body}</div>
                <div className="card-buttons">
                  <button
                    type="button"
                    disabled={adding !== null}
                    onClick={() => void addTile(tile.seed)}
                  >
                    {adding === tile.seed ? "Adding…" : "Add"}
                  </button>
                </div>
              </li>
            ))}
          </ul>
        </section>
      )}
    </section>
  );
}

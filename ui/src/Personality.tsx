import { useEffect, useState, type FormEvent } from "react";
import {
  deletePersonality,
  duplicatePersonality,
  getPersonalities,
  savePersonality,
  setExplicitPersonalities,
  setPersonality,
  type Emoji,
  type PersonalityEdit,
  type PersonalityPage,
  type Preset,
  type Verbosity,
} from "./api";
import { Toggle } from "./Toggle";
import { useNyxTiles } from "./nyx";
import { PRODUCT_NAME } from "./brand";

export const PERSONALITY_NOTE =
  "How replies sound in chat. Tone only: approvals, warnings and facts stay plain in every personality.";
export const TONE_RULES_MAX = 600;
export const ADULTS_ONLY =
  "Adults-only personalities: show explicit presets like Viper. Off hides them, and a chat set to one uses the default personality.";

const EMOJI: { value: Emoji; label: string }[] = [
  { value: "none", label: "None" },
  { value: "light", label: "Light" },
  { value: "heavy", label: "Heavy" },
];

const VERBOSITY: { value: Verbosity; label: string }[] = [
  { value: "terse", label: "Terse" },
  { value: "normal", label: "Normal" },
  { value: "chatty", label: "Chatty" },
];

function draftOf(p: Preset): PersonalityEdit {
  return {
    id: p.id,
    name: p.name,
    description: p.description,
    toneRules: p.toneRules,
    emoji: p.emoji,
    verbosity: p.verbosity,
  };
}

/** Settings → Personality: the cabin's presets and the person's own, from the
 * same files. Use makes it the default for every chat, from the next message. */
export function Personality() {
  const [page, setPage] = useState<PersonalityPage | null>(null);
  const [draft, setDraft] = useState<PersonalityEdit | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    getPersonalities().then(setPage, (err) => setNote(String(err)));
  }, []);

  async function run(work: () => Promise<PersonalityPage>, done: string) {
    setBusy(true);
    try {
      setPage(await work());
      setNote(done);
      return true;
    } catch (err) {
      setNote(String(err));
      return false;
    } finally {
      setBusy(false);
    }
  }

  function edit(p: Preset) {
    setDraft(draftOf(p));
    setConfirmDelete(false);
    setNote(null);
  }

  async function duplicate(p: Preset) {
    setBusy(true);
    try {
      const [id, next] = await duplicatePersonality(p.id);
      setPage(next);
      const copy = next.presets.find((q) => q.id === id);
      if (copy) edit(copy);
      setNote(`Made ${copy?.name ?? id}. Change it below.`);
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function save(event: FormEvent) {
    event.preventDefault();
    if (!draft) return;
    await run(() => savePersonality(draft), `Saved ${draft.name.trim()}.`);
  }

  async function remove() {
    if (!draft) return;
    if (!confirmDelete) {
      setConfirmDelete(true);
      return;
    }
    const name = draft.name;
    if (await run(() => deletePersonality(draft.id), `Deleted ${name}.`))
      setDraft(null);
  }

  const saved = draft && page?.presets.find((p) => p.id === draft.id);
  const tiles = useNyxTiles(page?.presets.map((p) => p.id) ?? null, page?.explicit ?? false);

  return (
    <div className="personality" role="group" aria-label="Personality">
      <p className="note">{PERSONALITY_NOTE}</p>
      {page && (
        <Toggle
          checked={page.explicit}
          disabled={busy}
          onChange={(on) =>
            void run(
              () => setExplicitPersonalities(on),
              on ? "Adults-only personalities on." : "Adults-only personalities off.",
            )
          }
        >
          {ADULTS_ONLY}
        </Toggle>
      )}
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
      {draft && (
        <form
          className="card persona-editor"
          aria-label={`Edit ${saved?.name ?? draft.name}`}
          onSubmit={(e) => void save(e)}
        >
          <div className="card-title">Edit {saved?.name ?? draft.name}</div>
          <p className="note">
            Saved as personalities/{draft.id}.toml in {PRODUCT_NAME}'s settings folder.
            Tone only: approvals and safety rules still come after it.
          </p>
          <label>
            Name
            <input
              value={draft.name}
              maxLength={32}
              onChange={(e) => setDraft({ ...draft, name: e.target.value })}
            />
          </label>
          <label>
            Description
            <input
              value={draft.description}
              maxLength={100}
              onChange={(e) =>
                setDraft({ ...draft, description: e.target.value })
              }
            />
          </label>
          <label>
            Tone rules ({draft.toneRules.length} of {TONE_RULES_MAX} characters)
            <textarea
              rows={4}
              maxLength={TONE_RULES_MAX}
              value={draft.toneRules}
              onChange={(e) =>
                setDraft({ ...draft, toneRules: e.target.value })
              }
            />
          </label>
          <label>
            Emoji
            <select
              value={draft.emoji}
              onChange={(e) =>
                setDraft({ ...draft, emoji: e.target.value as Emoji })
              }
            >
              {EMOJI.map((o) => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
            </select>
          </label>
          <label>
            Length
            <select
              value={draft.verbosity}
              onChange={(e) =>
                setDraft({ ...draft, verbosity: e.target.value as Verbosity })
              }
            >
              {VERBOSITY.map((o) => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
            </select>
          </label>
          <div className="card-buttons">
            <button type="submit" disabled={busy}>
              Save
            </button>
            <button
              type="button"
              className="quiet"
              disabled={busy || !saved}
              onClick={() => saved && setDraft(draftOf(saved))}
            >
              Reset
            </button>
            <button
              type="button"
              className="quiet"
              disabled={busy}
              onClick={() => void remove()}
            >
              {confirmDelete ? "Really delete" : "Delete"}
            </button>
            <button
              type="button"
              className="quiet"
              onClick={() => setDraft(null)}
            >
              Close
            </button>
          </div>
        </form>
      )}
      {page?.problems.map((problem) => (
        <p key={problem} className="note warn">
          Skipped {problem}
        </p>
      ))}
      {page === null && !note && (
        <p className="empty">Loading personalities…</p>
      )}
      {page && (
        <ul className="rows persona-cards" aria-label="Personalities">
          {page.presets.map((p) => {
            const on = p.id === page.current;
            return (
              <li key={p.id} className="card" aria-label={p.name}>
                {tiles[p.id] && (
                  <img className="persona-thumb" src={tiles[p.id]} alt="" aria-hidden="true" />
                )}
                <div className="card-status">
                  {on ? "In use" : p.custom ? "Yours" : "Built in"}
                </div>
                <div className="card-title">
                  {p.name}{" "}
                  {p.explicit && (
                    <span className="badge warn" title="Swears.">
                      Explicit
                    </span>
                  )}
                </div>
                <p className="card-detail-line">{p.description}</p>
                {p.sample && (
                  <p className="card-detail-line sample">“{p.sample}”</p>
                )}
                <div className="card-buttons">
                  {!on && (
                    <button
                      type="button"
                      disabled={busy}
                      onClick={() =>
                        void run(
                          () => setPersonality(p.id),
                          `Personality: ${p.name}`,
                        )
                      }
                    >
                      Use
                    </button>
                  )}
                  <button
                    type="button"
                    className="quiet"
                    disabled={busy}
                    onClick={() => void duplicate(p)}
                  >
                    Duplicate
                  </button>
                  {p.custom && (
                    <button
                      type="button"
                      className="quiet"
                      disabled={busy}
                      onClick={() => edit(p)}
                    >
                      Edit
                    </button>
                  )}
                </div>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}

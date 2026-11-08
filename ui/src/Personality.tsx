import { useEffect, useState, type FormEvent } from "react";
import {
  confirmAdult,
  deletePersonality,
  duplicatePersonality,
  getPersonalities,
  savePersonality,
  setDynamicChat,
  setExplicitPersonalities,
  setPersonality,
  setQuipHeat,
  type Emoji,
  type Heat,
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
export const ADULTS_ONLY = "Adults-only personalities";
export const ADULTS_ONLY_HINT =
  "18+ for every voice: crude jokes at the heat you pick, and explicit presets like Viper. Off keeps jokes clean and hides those presets; a chat set to one uses the default personality.";
/** The one-time 18+ check, the first time adults-only turns on (the cabin's wording). */
export const AGE_CHECK = `Adults-only personalities are for people 18 or older. Confirm once and ${PRODUCT_NAME} remembers it.`;
/** What turning 18+ on changes, shown with the age check. */
export const AGE_CHANGES =
  "What changes: every voice can tell crude, raunchy jokes at the heat you pick, and explicit presets like Viper show up. Still never sex chat in the regular voices, and never anything about minors, real people or non-consent.";
export const AGE_YES = "I'm 18 or older";
export const AGE_NO = "Not now";
export const DYNAMIC_CHAT = "Dynamic chat";
export const DYNAMIC_CHAT_HINT =
  "Another voice can step in while the topic calls for it, like Chief Engineer for a build error or Gentle Listener when you're venting, then hand back. A voice you pick for a chat stays put.";
export const HEAT = "Heat";
export const HEAT_HINT =
  'How crude the jokes get. You can also say "roast me harder" or "ease off" in a chat.';

/** The Heat dial's three stops, mildest first (`Heat::DIAL`). */
const HEAT_DIAL: { value: Heat; label: string }[] = [
  { value: "cheeky", label: "Cheeky" },
  { value: "spicy", label: "Spicy" },
  { value: "filthy", label: "Filthy" },
];

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
  /** The 18+ check is showing: the switch was turned on before it was answered. */
  const [ageCheck, setAgeCheck] = useState(false);

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

  /** Adults-only on asks the 18+ check first, once; off just turns it off. */
  async function adultsOnly(on: boolean) {
    setBusy(true);
    try {
      const next = await setExplicitPersonalities(on);
      setPage(next);
      setAgeCheck(next.ageCheck);
      setNote(
        next.ageCheck
          ? null
          : on
            ? "Adults-only personalities on."
            : "Adults-only personalities off.",
      );
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function answerAgeCheck(adult: boolean) {
    setAgeCheck(false);
    if (adult) await run(confirmAdult, "Adults-only personalities on.");
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
        <>
          <Toggle
            checked={page.explicit}
            disabled={busy}
            onChange={(on) => void adultsOnly(on)}
          >
            {ADULTS_ONLY}
          </Toggle>
          <p className="note">{ADULTS_ONLY_HINT}</p>
        </>
      )}
      {ageCheck && (
        <div className="card age-check" role="group" aria-label="Age check">
          <p className="note">{AGE_CHECK}</p>
          <p className="note">{AGE_CHANGES}</p>
          <div className="card-buttons">
            <button
              type="button"
              disabled={busy}
              onClick={() => void answerAgeCheck(true)}
            >
              {AGE_YES}
            </button>
            <button
              type="button"
              className="quiet"
              disabled={busy}
              onClick={() => void answerAgeCheck(false)}
            >
              {AGE_NO}
            </button>
          </div>
        </div>
      )}
      {page && (
        <>
          <Toggle
            checked={page.dynamicChat}
            disabled={busy}
            onChange={(on) =>
              void run(
                () => setDynamicChat(on),
                on ? "Dynamic chat on." : "Dynamic chat off: each chat keeps one voice.",
              )
            }
          >
            {DYNAMIC_CHAT}
          </Toggle>
          <p className="note">{DYNAMIC_CHAT_HINT}</p>
        </>
      )}
      {page?.explicit && (
        <fieldset className="channel heat">
          <legend>{HEAT}</legend>
          {HEAT_DIAL.map((stop) => (
            <label key={stop.value}>
              <input
                type="radio"
                name="quip-heat"
                checked={page.heat === stop.value}
                disabled={busy}
                onChange={() =>
                  void run(() => setQuipHeat(stop.value), `Heat: ${stop.label}`)
                }
              />
              {stop.label}
            </label>
          ))}
          <p className="note">{HEAT_HINT}</p>
        </fieldset>
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

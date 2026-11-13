import { useEffect, useEffectEvent, useId, useState } from "react";
import { homeCatalog, setHomeLayout, type HomeCatalog, type HomePick, type SlotSize } from "./api";

/** The seven slots, in the order Home draws them. */
export const SLOTS: { key: string; label: string; size: SlotSize }[] = [
  { key: "large", label: "Big tile", size: "large" },
  { key: "medium-0", label: "Middle tile 1", size: "medium" },
  { key: "medium-1", label: "Middle tile 2", size: "medium" },
  { key: "small-0", label: "Small tile 1", size: "small" },
  { key: "small-1", label: "Small tile 2", size: "small" },
  { key: "small-2", label: "Small tile 3", size: "small" },
  { key: "small-3", label: "Small tile 4", size: "small" },
];

function read(pick: HomePick, key: string): string {
  if (key === "large") return pick.large;
  const [kind, i] = key.split("-");
  return (kind === "medium" ? pick.medium[Number(i)] : pick.small[Number(i)]) ?? "";
}

/** `pick` with `id` in slot `key`, taken out of any other slot it was in. */
export function place(pick: HomePick, key: string, id: string): HomePick {
  const medium = [0, 1].map((i) => pick.medium[i] ?? "");
  const small = [0, 1, 2, 3].map((i) => pick.small[i] ?? null);
  if (id) {
    medium.forEach((m, i) => m === id && (medium[i] = ""));
    small.forEach((m, i) => m === id && (small[i] = null));
  }
  let large = pick.large === id && key !== "large" ? "" : pick.large;
  const [kind, i] = key.split("-");
  if (key === "large") large = id;
  else if (kind === "medium") medium[Number(i)] = id;
  else small[Number(i)] = id || null;
  return { preset: "custom", large, medium, small };
}

/** Edit Home: start from a preset, then pick what each slot shows. */
export function HomeEdit({ onDone }: { onDone: (saved: boolean) => void }) {
  const [cat, setCat] = useState<HomeCatalog | null>(null);
  const [pick, setPick] = useState<HomePick | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const ids = useId();

  // Runs each time the editor shows. A pick changed here and not saved stays;
  // untouched, it takes the saved layout.
  const refresh = useEffectEvent((c: HomeCatalog) => {
    setCat(c);
    if (!pick || pick === cat?.current) setPick(c.current);
  });
  useEffect(() => {
    homeCatalog().then(
      (c) => refresh(c),
      (err) => setNote(String(err)),
    );
  }, []);

  async function save() {
    if (!pick) return;
    setSaving(true);
    try {
      await setHomeLayout(pick);
      onDone(true);
    } catch (err) {
      setNote(String(err));
      setSaving(false);
    }
  }

  return (
    <section className="home-edit" aria-label="Edit Home">
      <h3>Edit Home</h3>
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
      {!cat && !note && <p className="empty">Loading…</p>}
      {cat && pick && (
        <>
          <div className="home-edit-presets" role="group" aria-label="Start from">
            {cat.presets.map((p) => (
              <button
                key={p.id}
                type="button"
                aria-pressed={pick.preset === p.id}
                onClick={() => setPick({ ...p.pick, preset: p.id })}
              >
                {p.label}
              </button>
            ))}
          </div>
          {SLOTS.map((slot) => {
            const fits = cat.modules.filter((m) => m.sizes.includes(slot.size));
            const now = read(pick, slot.key);
            const purpose = cat.modules.find((m) => m.id === now)?.purpose;
            return (
              <div key={slot.key} className="home-edit-slot">
                <label htmlFor={`${ids}-${slot.key}`}>{slot.label}</label>
                <select
                  id={`${ids}-${slot.key}`}
                  value={now}
                  onChange={(e) => setPick(place(pick, slot.key, e.target.value))}
                >
                  {slot.size !== "large" && <option value="">Nothing here</option>}
                  {fits.map((m) => (
                    <option key={m.id} value={m.id}>
                      {m.name}
                    </option>
                  ))}
                </select>
                {purpose && <small className="home-muted">{purpose}</small>}
              </div>
            );
          })}
          <div className="home-edit-actions">
            <button type="button" disabled={saving} onClick={() => void save()}>
              Save
            </button>
            <button type="button" className="quiet" disabled={saving} onClick={() => onDone(false)}>
              Cancel
            </button>
          </div>
        </>
      )}
    </section>
  );
}

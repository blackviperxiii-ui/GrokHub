import { useEffect, useRef, useState } from "react";
import { listModels, type ModelList, type ModelOption } from "./api";

export const SOURCE_LABELS: Record<ModelOption["source"], string> = {
  catalog: "",
  account: "Your account",
  custom: "Custom",
};

export const OTHER_HINT = "Type a model id, like grok-4.3";

/** The label for an id, from the list when it has one. */
export function modelLabel(id: string, list: ModelList | null): string {
  return list?.options.find((o) => o.id === id)?.label ?? id;
}

/** A model list with search and Other… to type an id. `value` is the model
 * in use; `onPick` gets the chosen id. `chip` draws it as a composer chip.
 * The list loads when it opens; Refresh reads the account's models again. */
export function ModelPicker({
  value,
  onPick,
  disabled = false,
  chip = false,
  labelledBy,
}: {
  value: string;
  onPick: (id: string) => void;
  disabled?: boolean;
  chip?: boolean;
  /** The id of the field label that names the button. */
  labelledBy?: string;
}) {
  const [open, setOpen] = useState(false);
  const [list, setList] = useState<ModelList | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [other, setOther] = useState<string | null>(null);
  const box = useRef<HTMLDivElement>(null);

  async function load(refresh: boolean) {
    setError(null);
    try {
      setList(await listModels(refresh));
    } catch (err) {
      setError(String(err));
    }
  }

  useEffect(() => {
    if (open) void load(false);
  }, [open]);

  useEffect(() => {
    if (!open) return;
    function onDown(event: MouseEvent) {
      if (box.current && !box.current.contains(event.target as Node)) close();
    }
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [open]);

  function close() {
    setOpen(false);
    setQuery("");
    setOther(null);
  }

  function pick(id: string) {
    const clean = id.trim();
    if (!clean) return;
    onPick(clean);
    close();
  }

  // Not a <form>: Settings → General wraps the picker in its own form.
  function pickTyped() {
    if (other !== null) pick(other);
  }

  const needle = query.trim().toLowerCase();
  const shown = (list?.options ?? []).filter(
    (o) => !needle || o.id.toLowerCase().includes(needle) || o.label.toLowerCase().includes(needle),
  );

  return (
    <div
      className="model-picker"
      ref={box}
      onKeyDown={(e) => {
        if (e.key === "Escape" && open) {
          e.preventDefault();
          e.stopPropagation();
          close();
        }
      }}
    >
      <button
        type="button"
        className={chip ? "chip" : "model-button"}
        aria-haspopup="dialog"
        aria-labelledby={labelledBy}
        aria-expanded={open}
        title={chip ? "Model for the next message" : undefined}
        disabled={disabled}
        onClick={() => (open ? close() : setOpen(true))}
      >
        {chip ? `Model: ${modelLabel(value, list)}` : modelLabel(value, list)}
      </button>
      {open && (
        <div className="model-pop" role="dialog" aria-label="Pick a model">
          <input
            aria-label="Search models"
            placeholder="Search models"
            autoFocus
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
          {error && <p className="note error-text">{error}</p>}
          {!list && !error && <p className="note">Loading models…</p>}
          {list && (
            <ul className="model-options" aria-label="Models">
              {shown.map((option) => (
                <li key={option.id}>
                  <button
                    type="button"
                    className="choice quiet"
                    aria-pressed={option.id === value}
                    onClick={() => pick(option.id)}
                  >
                    <span className="choice-label">{option.label}</span>
                    {option.label !== option.id && <code className="choice-detail">{option.id}</code>}
                    {SOURCE_LABELS[option.source] && (
                      <span className="badge">{SOURCE_LABELS[option.source]}</span>
                    )}
                  </button>
                </li>
              ))}
              {shown.length === 0 && <li className="note">No model matches.</li>}
            </ul>
          )}
          {list?.note && <p className="note">{list.note}</p>}
          {other === null ? (
            <div className="card-buttons">
              <button type="button" className="quiet" onClick={() => setOther(query.trim())}>
                Other…
              </button>
              <button type="button" className="quiet" onClick={() => void load(true)}>
                Refresh
              </button>
            </div>
          ) : (
            <div className="card-buttons">
              <input
                aria-label="Model id"
                placeholder={OTHER_HINT}
                autoFocus
                value={other}
                onChange={(e) => setOther(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    pickTyped();
                  }
                }}
              />
              <button type="button" disabled={!other.trim()} onClick={pickTyped}>
                Use
              </button>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

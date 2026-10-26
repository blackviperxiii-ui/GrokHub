import { useState } from "react";
import { setTheme, type Theme } from "./api";
import { applyTheme } from "./theme";

export const APPEARANCE_NOTE = "System follows your computer's light or dark setting.";

const CHOICES: { value: Theme; label: string }[] = [
  { value: "system", label: "System" },
  { value: "dark", label: "Dark" },
  { value: "light", label: "Light" },
];

/** Settings → Appearance. A pick shows at once and is saved in shell.json. */
export function Appearance({ theme, onSaved }: { theme: Theme; onSaved: (theme: Theme) => void }) {
  const [note, setNote] = useState<string | null>(null);

  async function choose(next: Theme) {
    applyTheme(next);
    try {
      const saved = await setTheme(next);
      onSaved(saved.theme);
      setNote(null);
    } catch (err) {
      applyTheme(theme);
      setNote(String(err));
    }
  }

  return (
    <div className="appearance" role="group" aria-label="Appearance">
      <p className="note">{APPEARANCE_NOTE}</p>
      <div className="chip-group" role="group" aria-label="Theme">
        {CHOICES.map((c) => (
          <button
            key={c.value}
            type="button"
            className="chip"
            aria-pressed={theme === c.value}
            onClick={() => void choose(c.value)}
          >
            {c.label}
          </button>
        ))}
      </div>
      {note && <p className="note warn">{note}</p>}
    </div>
  );
}

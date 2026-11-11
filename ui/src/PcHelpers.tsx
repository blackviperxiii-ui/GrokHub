import { useEffect, useState } from "react";
import {
  forgetPcRhythm,
  getPcHelpers,
  setPcAutoSleep,
  setPcHelper,
  type PcHelperKey,
  type PcHelpers as Shape,
} from "./api";
import { Toggle } from "./Toggle";

export const FOCUS_NOTE =
  "When you come back, the first suggestion fits the app you left (Blender, a slicer, a game, an editor). Reads only that app's process name, once, never window titles or contents. Off by default. Saved for this app; its suggestions don't show it yet.";
export const WIND_NOTE =
  "Sleep the PC after it sits idle the time you pick. Never during a game, a fullscreen app, a render, a download or an agent task.";
export const WIND_BLIND_NOTE =
  "This PC doesn't say whether a game, a fullscreen app or GPU work is running, so it won't sleep on its own.";
export const RHYTHM_NOTE =
  "Counts the minutes you're at this window, by weekday and hour, on this computer. Low-priority nudges wait for an hour you're usually around.";
export const GAMES_NOTE = "Adds each game's last-played time from your Steam library. Off by default.";

/** Settings → Your PC → PC helpers: the wellbeing and focus switches the
 * cabin has, saved in the same files so both apps agree. */
export function PcHelpers() {
  const [page, setPage] = useState<Shape | null>(null);
  const [note, setNote] = useState<string | null>(null);

  useEffect(() => {
    getPcHelpers().then(setPage, (err) => setNote(String(err)));
  }, []);

  async function save(job: Promise<Shape>) {
    try {
      setPage(await job);
      setNote("Saved.");
    } catch (err) {
      setNote(String(err));
    }
  }

  if (!page) return <p className="note">{note ?? "Loading…"}</p>;
  const flip = (key: PcHelperKey) => (on: boolean) => void save(setPcHelper(key, on));

  return (
    <div className="behavior" role="group" aria-label="PC helpers">
      <Toggle checked={page.focusHelp} onChange={flip("focusHelp")}>
        Help with the app I was just in
      </Toggle>
      <p className="note">{FOCUS_NOTE}</p>

      <Toggle checked={page.windDown} onChange={flip("windDown")}>
        Sleep when idle
      </Toggle>
      <p className="note">{WIND_NOTE}</p>
      {page.windDown && (
        <>
          <div className="chip-group" role="group" aria-label="Sleep after">
            {page.sleepChoices.map((c) => (
              <button
                key={c.minutes}
                type="button"
                className="chip"
                aria-pressed={page.autoSleepMin === c.minutes}
                onClick={() => void save(setPcAutoSleep(c.minutes))}
              >
                {c.label}
              </button>
            ))}
          </div>
          {page.sleepBlind && <p className="note">{WIND_BLIND_NOTE}</p>}
        </>
      )}

      <Toggle checked={page.rhythm} onChange={flip("rhythm")}>
        Learn my daily rhythm
      </Toggle>
      <p className="note">{RHYTHM_NOTE}</p>
      {page.rhythm && (
        <>
          <Toggle checked={page.rhythmGames} onChange={flip("rhythmGames")}>
            Count when I last played
          </Toggle>
          <p className="note">{GAMES_NOTE}</p>
          <p className="note">{page.rhythmSummary}</p>
          {page.rhythmMinutes > 0 && (
            <button type="button" onClick={() => void save(forgetPcRhythm())}>
              Forget my rhythm
            </button>
          )}
        </>
      )}
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
    </div>
  );
}

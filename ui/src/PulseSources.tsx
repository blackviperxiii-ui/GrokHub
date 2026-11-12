import { useEffect, useState } from "react";
import {
  getPulseSources,
  savePulseSources,
  type PulseSourcesView,
} from "./api";
import { Toggle } from "./Toggle";

const DROP_SOURCES = [
  { key: "hackerNews", label: "Hacker News" },
  { key: "reddit", label: "Reddit" },
  { key: "webSearch", label: "Web search" },
  { key: "github", label: "GitHub" },
  { key: "youtube", label: "YouTube" },
  { key: "rss", label: "RSS" },
] as const;

export const SOURCES_NOTE =
  "What Pulse writes on its own: the morning brief, the Sunday recaps, the overnight daily drop, release news for games you own, hobby tips, and offers to automate things you ask for again and again. Same switches as the cabin.";

/** Settings → Pulse: the digest and offer sources, cabin defaults. */
export function PulseSources() {
  const [view, setView] = useState<PulseSourcesView | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    getPulseSources().then(setView, (err) => setNote(String(err)));
  }, []);

  async function save(next: PulseSourcesView) {
    setView(next);
    setBusy(true);
    try {
      setView(await savePulseSources(next));
      setNote(null);
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
    }
  }

  if (!view) {
    return (
      <div className="interests" role="group" aria-label="Pulse sources">
        <h2>Sources</h2>
        {note ? <p className="note">{note}</p> : <p className="note">Loading…</p>}
      </div>
    );
  }

  return (
    <div className="interests" role="group" aria-label="Pulse sources">
      <h2>Sources</h2>
      <p className="note">{SOURCES_NOTE}</p>
      <ul className="labs-list" aria-label="Source switches">
        <li aria-label="Daily drop">
          <Toggle
            checked={view.dailyDrop.enabled}
            disabled={busy}
            onChange={(enabled) =>
              void save({ ...view, dailyDrop: { ...view.dailyDrop, enabled } })
            }
          >
            Daily drop — a few ranked picks each morning
          </Toggle>
        </li>
        {DROP_SOURCES.map(({ key, label }) => (
          <li key={key} aria-label={`Daily drop ${label}`}>
            <Toggle
              checked={view.dailyDrop[key]}
              disabled={busy || !view.dailyDrop.enabled}
              onChange={(on) =>
                void save({
                  ...view,
                  dailyDrop: { ...view.dailyDrop, [key]: on },
                })
              }
            >
              {label}
            </Toggle>
          </li>
        ))}
        <li aria-label="Release radar">
          <Toggle
            checked={view.releaseRadarOn}
            disabled={busy}
            onChange={(releaseRadarOn) => void save({ ...view, releaseRadarOn })}
          >
            Release radar — DLC and sequels for games you own or want
          </Toggle>
        </li>
        <li aria-label="Automate offers">
          <Toggle
            checked={view.askOffersEnabled}
            disabled={busy}
            onChange={(askOffersEnabled) =>
              void save({ ...view, askOffersEnabled })
            }
          >
            Automate offers — ask to automate repeated requests
          </Toggle>
        </li>
        <li aria-label="Hobby tips">
          <Toggle
            checked={view.hobbyPaths.tips}
            disabled={busy}
            onChange={(tips) =>
              void save({ ...view, hobbyPaths: { ...view.hobbyPaths, tips } })
            }
          >
            Hobby tips — Know me learning paths
          </Toggle>
        </li>
        <li aria-label="Hobby projects">
          <Toggle
            checked={view.hobbyPaths.projects}
            disabled={busy}
            onChange={(projects) =>
              void save({
                ...view,
                hobbyPaths: { ...view.hobbyPaths, projects },
              })
            }
          >
            Hobby project ideas
          </Toggle>
        </li>
      </ul>
      {note && <p className="note">{note}</p>}
    </div>
  );
}

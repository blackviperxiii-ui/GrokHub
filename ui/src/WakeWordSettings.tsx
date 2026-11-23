import { useEffect, useState } from "react";
import {
  getWakeWord,
  wakeWordAct,
  type WakeWordPage,
  type WakeWordRoute,
} from "./api";

function linesOf(text: string): string[] {
  return text
    .split("\n")
    .map((s) => s.trim())
    .filter(Boolean);
}

function routesFromText(text: string): WakeWordRoute[] {
  const out: WakeWordRoute[] = [];
  for (const line of linesOf(text)) {
    const sep = line.indexOf("=");
    if (sep <= 0) continue;
    const phrase = line.slice(0, sep).trim();
    const profileId = line.slice(sep + 1).trim();
    if (phrase) out.push({ phrase, profileId });
  }
  return out;
}

function routesToText(routes: WakeWordRoute[]): string {
  return routes.map((r) => `${r.phrase}=${r.profileId}`).join("\n");
}

export function WakeWordSettings() {
  const [page, setPage] = useState<WakeWordPage | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [phrasesText, setPhrasesText] = useState("");
  const [stopText, setStopText] = useState("");
  const [routesText, setRoutesText] = useState("");
  const [profile, setProfile] = useState("");

  useEffect(() => {
    if (typeof getWakeWord !== "function") return;
    let cancelled = false;
    void getWakeWord()
      .then((next) => {
        if (cancelled) return;
        setPage(next);
        setPhrasesText(next.phrases.join("\n"));
        setStopText(next.stopPhrases.join("\n"));
        setRoutesText(routesToText(next.routes));
        setProfile(next.defaultProfile);
      })
      .catch((err: unknown) => {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  if (!page) return null;

  async function save(act: Parameters<typeof wakeWordAct>[0]) {
    setError(null);
    try {
      const next = await wakeWordAct(act);
      setPage(next);
      setPhrasesText(next.phrases.join("\n"));
      setStopText(next.stopPhrases.join("\n"));
      setRoutesText(routesToText(next.routes));
      setProfile(next.defaultProfile);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  const phraseHint =
    page.phrases.length === 0
      ? `Defaults: ${page.defaultPhrases.join(", ")}`
      : "One wake phrase per line. Matching stays on this computer.";
  const stopHint =
    page.stopPhrases.length === 0
      ? `Defaults: ${page.defaultStop.join(", ")}`
      : "One stop phrase per line. Spoken during a wake-started turn.";

  return (
    <div className="local-engine" aria-label="Wake word">
      <h3>Wake word</h3>
      <p className="note">{page.privacy}</p>
      <div className="local-mode" role="group" aria-label="Wake word mic consent">
        <button
          type="button"
          className="quiet"
          aria-pressed={page.micConsent}
          onClick={() => void save({ kind: "setMicConsent", on: !page.micConsent })}
        >
          Allow mic for wake word
        </button>
        <button
          type="button"
          className="quiet"
          aria-pressed={page.enabled}
          disabled={!page.micConsent}
          onClick={() => void save({ kind: "setEnabled", on: !page.enabled })}
        >
          Listen for wake phrases
        </button>
        <button
          type="button"
          className="quiet"
          aria-pressed={page.phase === "armed" || page.phase === "in_turn"}
          disabled={!page.micConsent || !page.enabled}
          onClick={() =>
            void save(
              page.phase === "armed" || page.phase === "in_turn"
                ? { kind: "disarm" }
                : { kind: "arm" },
            )
          }
        >
          {page.phase === "armed" || page.phase === "in_turn" ? "Stop arming" : "Arm listening"}
        </button>
      </div>
      <p className="note">
        Phase: {page.phase}
        {page.voiceBusy ? " · paused while voice mode owns the mic" : ""}
      </p>
      <label className="note">
        Wake phrases
        <textarea
          aria-label="Wake phrases"
          rows={3}
          value={phrasesText}
          placeholder={page.defaultPhrases.join("\n")}
          onChange={(e) => setPhrasesText(e.target.value)}
          onBlur={() => void save({ kind: "setPhrases", phrases: linesOf(phrasesText) })}
        />
      </label>
      <p className="note">{phraseHint}</p>
      <label className="note">
        Stop phrases
        <textarea
          aria-label="Stop phrases"
          rows={2}
          value={stopText}
          placeholder={page.defaultStop.join("\n")}
          onChange={(e) => setStopText(e.target.value)}
          onBlur={() => void save({ kind: "setStopPhrases", phrases: linesOf(stopText) })}
        />
      </label>
      <p className="note">{stopHint}</p>
      <label className="note">
        Profile routes (phrase=personality id)
        <textarea
          aria-label="Wake profile routes"
          rows={2}
          value={routesText}
          placeholder={"hey cabin=nyx"}
          onChange={(e) => setRoutesText(e.target.value)}
          onBlur={() => void save({ kind: "setRoutes", routes: routesFromText(routesText) })}
        />
      </label>
      <label className="note">
        Default profile
        <input
          aria-label="Default wake profile"
          value={profile}
          placeholder="neutral"
          onChange={(e) => setProfile(e.target.value)}
          onBlur={() => void save({ kind: "setDefaultProfile", profileId: profile })}
        />
      </label>
      {error && (
        <p className="note error" role="status">
          {error}
        </p>
      )}
    </div>
  );
}

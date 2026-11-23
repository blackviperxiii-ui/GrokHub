import { useEffect, useState } from "react";
import { getVoiceRoute, setVoiceRoute, type VoiceRouteConfig, type VoiceRouteMode } from "./api";

// No per-stage source picker: Auto picks speech to text, the reply model and
// the voice. Local only is the privacy setting that keeps all three here.
const MODE_OPTIONS: readonly [VoiceRouteMode, string, string][] = [
  [
    "auto",
    "Auto",
    "Auto picks between on-device and online sources from connection, sign-in, and installed models.",
  ],
  [
    "local_only",
    "Local only",
    "Local only keeps speech to text, replies, and spoken voice on this computer.",
  ],
];

export function VoiceRouteSettings() {
  const [config, setConfig] = useState<VoiceRouteConfig | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (typeof getVoiceRoute !== "function") return;
    let cancelled = false;
    void getVoiceRoute()
      .then((next) => {
        if (!cancelled) setConfig(next);
      })
      .catch((err: unknown) => {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  if (!config) return null;

  async function save(next: VoiceRouteConfig) {
    setError(null);
    setConfig(next);
    try {
      const saved = await setVoiceRoute(next);
      setConfig(saved);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  const activeHint = MODE_OPTIONS.find(([mode]) => mode === config.mode)?.[2] ?? "";

  return (
    <div className="local-engine" aria-label="Voice routing">
      <h3>Voice routing</h3>
      <p className="note">{activeHint}</p>
      <div className="local-mode" role="group" aria-label="Voice routing mode">
        {MODE_OPTIONS.map(([mode, label]) => (
          <button
            key={mode}
            type="button"
            className="quiet"
            aria-pressed={config.mode === mode}
            onClick={() => void save({ ...config, mode })}
          >
            {label}
          </button>
        ))}
      </div>
      {error && (
        <p className="note error" role="status">
          {error}
        </p>
      )}
    </div>
  );
}

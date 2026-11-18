import { useEffect, useState } from "react";
import {
  getAiModels,
  getVoiceRoute,
  setVoiceRoute,
  type AiProviderRow,
  type SpeechSource,
  type VoiceRouteConfig,
  type VoiceRouteMode,
} from "./api";

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
  [
    "pinned",
    "Pinned",
    "Pinned uses the source you choose for each stage.",
  ],
];

function parseSpeechSource(value: string): SpeechSource | null {
  if (value === "local" || value === "grok_voice_3") return value;
  return null;
}

export function VoiceRouteSettings() {
  const [config, setConfig] = useState<VoiceRouteConfig | null>(null);
  const [providers, setProviders] = useState<AiProviderRow[]>([]);
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
    if (typeof getAiModels === "function") {
      void getAiModels()
        .then((view) => {
          if (!cancelled) setProviders(view.providers);
        })
        .catch(() => {});
    }
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
  const replyProviders = providers.filter(
    (p) =>
      p.id !== "local" &&
      p.id !== "xai" &&
      (p.ready || (config.pinned.reply !== null && p.id === config.pinned.reply)),
  );
  const extraReply =
    config.pinned.reply &&
    config.pinned.reply !== "local" &&
    config.pinned.reply !== "xai" &&
    !replyProviders.some((p) => p.id === config.pinned.reply)
      ? config.pinned.reply
      : null;

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
      {config.mode === "pinned" && (
        <div className="local-grid" role="group" aria-label="Pinned voice stages">
          <label>
            Speech to text
            <select
              value={config.pinned.stt ?? ""}
              onChange={(e) =>
                void save({
                  ...config,
                  pinned: { ...config.pinned, stt: parseSpeechSource(e.target.value) },
                })
              }
            >
              <option value="">Auto</option>
              <option value="local">On-device</option>
              <option value="grok_voice_3">Grok Voice 3</option>
            </select>
          </label>
          <label>
            Reply model
            <select
              value={config.pinned.reply ?? ""}
              onChange={(e) =>
                void save({
                  ...config,
                  pinned: { ...config.pinned, reply: e.target.value || null },
                })
              }
            >
              <option value="">Auto</option>
              <option value="local">On-device</option>
              <option value="xai">xAI</option>
              {replyProviders.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.id}
                </option>
              ))}
              {extraReply && <option value={extraReply}>{extraReply}</option>}
            </select>
          </label>
          <label>
            Voice / TTS
            <select
              value={config.pinned.tts ?? ""}
              onChange={(e) =>
                void save({
                  ...config,
                  pinned: { ...config.pinned, tts: parseSpeechSource(e.target.value) },
                })
              }
            >
              <option value="">Auto</option>
              <option value="local">On-device</option>
              <option value="grok_voice_3">Grok Voice 3</option>
            </select>
          </label>
        </div>
      )}
      {error && (
        <p className="note error" role="status">
          {error}
        </p>
      )}
    </div>
  );
}

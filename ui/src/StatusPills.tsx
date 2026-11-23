// The top bar's pills: "Quiet until 07:00" while quiet hours hold, the
// default personality, and how much this week left the computer. Each opens
// its Settings section. A pill whose answer is missing (a command this
// desktop or the web app doesn't have, a locked keyring) says less instead of
// failing.
import { useEffect, useState } from "react";
import { getPersonalities, getPrivacy, quietChip } from "./api";
import type { Section } from "./sections";

/** The personality pill's value: the default voice's name, or Home's fallback. */
export function personalityName(page: { current: string; presets: { id: string; name: string }[] }): string {
  return page.presets.find((preset) => preset.id === page.current)?.name || "Default voice";
}

/** The local pill's text: calls, never bytes, as Privacy & data counts them. */
export function localText(page: { offlineOnly: boolean; sentCalls?: number | null } | null): string {
  const calls = page?.sentCalls;
  const count = typeof calls === "number" ? calls : null;
  if (page?.offlineOnly) return count === null ? "Offline only" : `Offline only · ${count} sent`;
  if (count === 0) return "Local · 0 sent";
  return count === null ? "Local first" : `Local first · ${count} sent`;
}

const MINUTE = 60_000;

export function StatusPills({
  remote,
  refresh,
  onSettings,
}: {
  /** The web app may not read the personalities; it shows the local pill only. */
  remote: boolean;
  /** Changes when the pills should read again: Settings closed, a turn ended. */
  refresh: string;
  onSettings: (section: Section) => void;
}) {
  const [name, setName] = useState<string | null>(null);
  const [local, setLocal] = useState<{ text: string; title: string } | null>(null);
  const [quiet, setQuiet] = useState<string | null>(null);

  // Quiet hours start and end on the minute, so the pill reads again at each
  // minute's turn as well as when Settings closes.
  useEffect(() => {
    if (remote) return;
    let live = true;
    let timer = 0;
    const read = () => {
      Promise.resolve()
        .then(() => quietChip())
        .then(
          (text) => {
            if (live) setQuiet(text ?? null);
          },
          () => {
            if (live) setQuiet(null);
          },
        );
      timer = window.setTimeout(read, MINUTE - (Date.now() % MINUTE) + 50);
    };
    read();
    return () => {
      live = false;
      window.clearTimeout(timer);
    };
  }, [remote, refresh]);

  useEffect(() => {
    let live = true;
    if (!remote) {
      Promise.resolve()
        .then(() => getPersonalities())
        .then(
          (page) => {
            if (live) setName(personalityName(page));
          },
          () => {
            if (live) setName(null);
          },
        );
    }
    Promise.resolve()
      .then(() => getPrivacy())
      .then(
        (page) => {
          if (live) setLocal({ text: localText(page), title: page.sent });
        },
        () => {
          if (live) setLocal(null);
        },
      );
    return () => {
      live = false;
    };
  }, [remote, refresh]);

  const localNow = local?.text ?? localText(null);
  const [key, ...rest] = localNow.split(" · ");
  const voice = name ? `Personality · ${name}` : "Personality";
  return (
    <div className="pills">
      {quiet && (
        <button
          type="button"
          className="pill pill-quiet"
          title="Automations wait until then. Opens Behavior."
          onClick={() => onSettings("behavior")}
        >
          <span className="pill-dot" aria-hidden="true" />
          <span className="pill-key">{quiet}</span>
        </button>
      )}
      {!remote && (
        <button
          type="button"
          className="pill pill-personality"
          aria-label={voice}
          onClick={() => onSettings("personality")}
        >
          <span className="pill-dot" aria-hidden="true" />
          <span className="pill-key">Personality</span>
          {name && (
            <>
              <span className="pill-sep" aria-hidden="true">
                ·
              </span>
              <span className="pill-value">{name}</span>
            </>
          )}
        </button>
      )}
      <button
        type="button"
        className="pill pill-local"
        aria-label={localNow}
        title={local?.title ?? "Calls sent off-device this week"}
        onClick={() => onSettings("privacy")}
      >
        <span className="pill-dot" aria-hidden="true" />
        <span className="pill-key">{key}</span>
        {rest.length > 0 && (
          <>
            <span className="pill-sep" aria-hidden="true">
              ·
            </span>
            <span className="pill-value">{rest.join(" · ")}</span>
          </>
        )}
      </button>
    </div>
  );
}

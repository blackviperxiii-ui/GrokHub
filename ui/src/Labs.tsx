import { Fragment, useEffect, useEffectEvent, useState, type FormEvent } from "react";
import {
  fetchPluginMarket,
  getPlugins,
  installPlugin,
  pluginAct,
  type PluginAct,
  type PluginEntry,
  type PluginPage,
  type PluginRow,
} from "./api";
import { COMPANION_NAME } from "./brand";
import { desktopNyxSet } from "./desktopNyxApi";
import { Diagnostics } from "./Diagnostics";
import { Toggle } from "./Toggle";
import { inShell } from "./transport";
import { useDesktopNyx } from "./useDesktopNyx";
import { StatusLine, useNote } from "./ui";

export const LABS_NOTE = "Early features. They work, but they may change.";
export const CHANNEL_NOTE = "Switch between beta and stable in About, under Updates.";
export const CHANNEL_NOTE_REMOTE =
  "Switch between beta and stable on your computer, in About, under Updates.";
export const PLUGINS_TIP =
  "A preview: plugins work, but how they install and trust may change.";
export const PLUGINS_MCP_NOTE = "MCP servers you add yourself are on the Connectors page.";
export const INSTALL_HINT = "Nothing from the bundle runs. It installs off and untrusted.";
export const MARKET_HINT = "An https index URL. Fetch only lists entries; Install is a separate step.";
export const MARKET_EMPTY = "No marketplace entries.";
export const REMOVE_HINT = "Remove the installed copy.";
export const PLUGINS_NOTE =
  "Bundles stay off until you trust this version and turn them on. Trust covers the hook commands, MCP server commands, skills and agents a bundle lists. Fetching a marketplace list installs nothing.";
export const PLUGINS_EMPTY =
  "No plugins yet. Install a bundle above. It stays off until you trust it and turn it on.";
export const DESKTOP_NYX_TIP = `Puts a pop-out on ${COMPANION_NAME}'s dock. Popped out, she stands on a holo pedestal in a small see-through window over your desktop.`;
export const DESKTOP_NYX_GAME = "On the desktop, hidden while you game.";
export const DESKTOP_NYX_WAYLAND =
  "On this desktop, a see-through always-on-top window may not be honoured (common on Wayland). She still opens; the glass and float may not.";

/** The next step for a bundle: trust first, then on and off. */
function next(row: PluginRow): { act: PluginAct; label: string } {
  if (!row.trusted) return { act: "trust", label: "Trust" };
  return row.enabled
    ? { act: "disable", label: "Turn off" }
    : { act: "enable", label: "Turn on" };
}

function state(row: PluginRow) {
  const base = !row.trusted ? "Not trusted · off" : row.enabled ? "Trusted · on" : "Trusted · off";
  if (row.parts.some((p) => p.warn && p.label === "Can't be trusted")) return `${base} · can't be trusted`;
  if (row.parts.some((p) => p.warn)) return `${base} · has refused parts`;
  return base;
}

/** Settings → Labs: early features, and the plugin bundles with their trust and on/off state. */
export function Labs({ onOpenConnectors }: { onOpenConnectors?: () => void } = {}) {
  const [page, setPage] = useState<PluginPage | null>(null);
  const [source, setSource] = useState("");
  const [marketUrl, setMarketUrl] = useState("");
  const [entries, setEntries] = useState<PluginEntry[]>([]);
  /** A fetch came back, so an empty list means the index has none. */
  const [fetched, setFetched] = useState(false);
  const { note, ok, fail } = useNote();
  const [busy, setBusy] = useState(false);

  // Runs each time Labs shows; a market address typed here and not used stays.
  const refresh = useEffectEvent((loaded: PluginPage) => {
    if (!page || marketUrl === page.marketUrl) setMarketUrl(loaded.marketUrl);
    setPage(loaded);
  });
  useEffect(() => {
    getPlugins().then(
      (loaded) => refresh(loaded),
      fail,
    );
  }, [fail]);

  async function run(work: () => Promise<PluginPage>) {
    setBusy(true);
    try {
      const done = await work();
      setPage(done);
      ok(done.note || null);
      return done;
    } catch (err) {
      fail(err);
      return null;
    } finally {
      setBusy(false);
    }
  }

  async function install(event: { preventDefault(): void }, from: string) {
    event.preventDefault();
    if (!from.trim() || busy) return;
    const done = await run(() => installPlugin(from));
    if (done?.note.startsWith("Installed")) setSource("");
  }

  async function fetchList(event: FormEvent) {
    event.preventDefault();
    if (busy) return;
    const done = await run(async () => {
      const market = await fetchPluginMarket(marketUrl);
      setEntries(market.entries);
      setFetched(market.read);
      setMarketUrl(market.page.marketUrl);
      return market.page;
    });
    // A fetch that fails reads nothing, so it never keeps an earlier
    // "empty" or that list's entries. The list stays up while one runs.
    if (done === null) {
      setFetched(false);
      setEntries([]);
    }
  }

  return (
    <div className="labs" role="group" aria-label="Labs">
      <h2>Labs</h2>
      <p className="note">{LABS_NOTE}</p>
      <p className="note">{inShell() ? CHANNEL_NOTE : CHANNEL_NOTE_REMOTE}</p>
      {inShell() && <DesktopCompanion />}
      <h3>
        Plugins{" "}
        <span className="badge" title={PLUGINS_TIP}>
          Preview
        </span>
      </h3>
      <p className="note">{PLUGINS_NOTE}</p>
      <p className="note">
        {onOpenConnectors ? (
          <>
            MCP servers you add yourself are on the{" "}
            <button type="button" className="link" onClick={onOpenConnectors}>
              Connectors
            </button>{" "}
            page.
          </>
        ) : (
          PLUGINS_MCP_NOTE
        )}
      </p>
      <form className="labs-row" onSubmit={(e) => void install(e, source)}>
        <label>
          Install a bundle
          <input
            value={source}
            placeholder="https, ssh or file URL, or a folder path"
            onChange={(e) => setSource(e.target.value)}
          />
        </label>
        <button type="submit" disabled={busy || !source.trim()}>
          Install
        </button>
      </form>
      <p className="note">{INSTALL_HINT}</p>
      <StatusLine note={note} className="note" />
      {page === null && !note && <p className="empty">Loading plugins…</p>}
      {page && page.rows.length === 0 && (
        <p className="empty">{PLUGINS_EMPTY}</p>
      )}
      {page && page.rows.length > 0 && (
        <ul className="rows" aria-label="Installed plugins">
          {page.rows.map((row) => (
            <li key={row.name} className="card" aria-label={row.name}>
              <div className="card-status">{state(row)}</div>
              <div className="card-title">
                {row.name} {row.version}
              </div>
              {row.description && (
                <p className="card-detail-line plain plugin-desc">{row.description}</p>
              )}
              {row.parts
                .filter((part) => !part.label)
                .map((part, i) => (
                  <p key={i} className="card-detail-line plain plugin-whole">
                    {part.text}
                  </p>
                ))}
              {row.parts.some((part) => part.label) && (
                <dl className="plugin-parts">
                  {row.parts
                    .filter((part) => part.label)
                    .map((part, i) => (
                      <Fragment key={i}>
                        <dt>{part.label}</dt>
                        <dd className={part.warn ? "warn" : undefined}>{part.text}</dd>
                      </Fragment>
                    ))}
                </dl>
              )}
              <div className="card-buttons">
                <button
                  type="button"
                  disabled={busy}
                  onClick={() =>
                    void run(() => pluginAct(row.name, next(row).act))
                  }
                >
                  {next(row).label}
                </button>
                {row.installed && (
                  <button
                    type="button"
                    className="quiet"
                    disabled={busy}
                    onClick={() =>
                      void run(() => pluginAct(row.name, "remove"))
                    }
                  >
                    Remove
                  </button>
                )}
              </div>
              {row.installed && <p className="note">{REMOVE_HINT}</p>}
            </li>
          ))}
        </ul>
      )}
      <form className="labs-row" onSubmit={(e) => void fetchList(e)}>
        <label>
          Marketplace index
          <input
            value={marketUrl}
            placeholder="https://… index URL"
            onChange={(e) => setMarketUrl(e.target.value)}
          />
        </label>
        <button type="submit" disabled={busy}>
          Fetch list
        </button>
      </form>
      <p className="note">{MARKET_HINT}</p>
      {fetched && entries.length === 0 && <p className="empty">{MARKET_EMPTY}</p>}
      {entries.length > 0 && (
        <ul className="rows" aria-label="Marketplace">
          {entries.map((entry) => (
            <li key={entry.source} className="card" aria-label={entry.name}>
              <div className="card-title">
                {entry.name} {entry.version}
              </div>
              {entry.description && (
                <p className="card-detail-line">{entry.description}</p>
              )}
              <code className="card-detail-line">{entry.source}</code>
              <div className="card-buttons">
                <button
                  type="button"
                  disabled={busy}
                  onClick={(e) => void install(e, entry.source)}
                >
                  Install
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}
      <Diagnostics />
    </div>
  );
}

/** Settings → Labs → Desktop companion. Off by default. */
function DesktopCompanion() {
  const { fail } = useNote();
  // The tray and the desk page change it too; their events land here.
  const [desk, setDesk] = useDesktopNyx(true, fail);
  if (!desk) return null;
  const effectsOk = desk.effects.transparent && desk.effects.onTop;
  return (
    <div className="labs-desktop-nyx" role="group" aria-label={`Desktop ${desk.companion}`}>
      <h3>
        Desktop {desk.companion}{" "}
        <span className="badge" title={DESKTOP_NYX_TIP}>
          Preview
        </span>
      </h3>
      <Toggle
        checked={desk.enabled}
        onChange={(on) => void desktopNyxSet({ enabled: on }).then(setDesk, fail)}
      >
        Desktop companion
      </Toggle>
      <p className="note">{DESKTOP_NYX_TIP}</p>
      {!effectsOk && <p className="note warn">{DESKTOP_NYX_WAYLAND}</p>}
      {desk.enabled && (
        <>
          <Toggle
            checked={desk.behind}
            onChange={(behind) => void desktopNyxSet({ behind }).then(setDesk, fail)}
          >
            Stay behind windows
          </Toggle>
          <p className="note">
            On: she sits on the desktop and your apps cover her. Off: she floats above them.
          </p>
          <Toggle
            checked={desk.adult}
            onChange={(adult) => void desktopNyxSet({ adult }).then(setDesk, fail)}
          >
            Allow 18+ outfits on the desktop
          </Toggle>
          <p className="note">
            Off by default: the desktop is visible to anyone at the screen. Needs adults-only
            personalities on.
          </p>
          <p className="note">
            {!desk.undocked
              ? "Docked in the chat. Use Put her on the desktop on her dock."
              : desk.hiddenForGame
                ? DESKTOP_NYX_GAME
                : "On the desktop. Use Bring her back on her dock or right-click menu."}
          </p>
        </>
      )}
    </div>
  );
}

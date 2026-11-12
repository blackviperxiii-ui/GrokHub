import { useEffect, useEffectEvent, useState, type FormEvent } from "react";
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
import { COMPANION_NAME, PRODUCT_NAME } from "./brand";
import {
  desktopNyxGet,
  desktopNyxSet,
  type DesktopNyxState,
} from "./desktopNyxApi";
import { Diagnostics } from "./Diagnostics";
import { Toggle } from "./Toggle";
import { inShell } from "./transport";
import { StatusLine, useNote } from "./ui";

export const LABS_NOTE = "Early features. They work, but they may change.";
export const CHANNEL_NOTE =
  `The beta channel switch is in the ${PRODUCT_NAME} cabin. This app updates from its release downloads.`;
export const PLUGINS_TIP =
  "A preview: the same plugins as the cabin's Labs page, with fewer details on each bundle.";
export const PLUGINS_NOTE =
  "Bundles stay off until you trust this version and turn them on. Trust covers the hook commands, MCP server commands, skills and agents a bundle lists. Fetching a marketplace list installs nothing.";
export const PLUGINS_EMPTY =
  "No plugins yet. Install a bundle above. It stays off until you trust it and turn it on.";
export const DESKTOP_NYX_TIP = `Puts a pop-out on ${COMPANION_NAME}'s dock. Popped out, she stands on a holo pedestal in a small see-through window over your desktop.`;
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
  if (!row.trusted) return "Not trusted · off";
  return row.enabled ? "Trusted · on" : "Trusted · off";
}

/** Settings → Labs: plugins, from the same files as the cabin's Labs page. */
export function Labs() {
  const [page, setPage] = useState<PluginPage | null>(null);
  const [source, setSource] = useState("");
  const [marketUrl, setMarketUrl] = useState("");
  const [entries, setEntries] = useState<PluginEntry[]>([]);
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
    await run(async () => {
      const market = await fetchPluginMarket(marketUrl);
      setEntries(market.entries);
      setMarketUrl(market.page.marketUrl);
      return market.page;
    });
  }

  return (
    <div className="labs" role="group" aria-label="Labs">
      <h2>Labs</h2>
      <p className="note">{LABS_NOTE}</p>
      <p className="note">{CHANNEL_NOTE}</p>
      {inShell() && <DesktopCompanion />}
      <h3>
        Plugins{" "}
        <span className="badge" title={PLUGINS_TIP}>
          Preview
        </span>
      </h3>
      <p className="note">{PLUGINS_NOTE}</p>
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
              <pre className="card-detail plain">{row.summary}</pre>
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

/** Settings → Labs → Desktop companion (cabin desktop_nyx). Off by default. */
function DesktopCompanion() {
  const [desk, setDesk] = useState<DesktopNyxState | null>(null);
  const { fail } = useNote();
  useEffect(() => {
    void desktopNyxGet().then(setDesk, fail);
  }, [fail]);
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
            {desk.undocked
              ? "On the desktop. Use Bring her back on her dock or right-click menu."
              : "Docked in the chat. Use Put her on the desktop on her dock."}
          </p>
        </>
      )}
    </div>
  );
}

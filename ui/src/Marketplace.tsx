import { Fragment, useEffect, useEffectEvent, useMemo, useState, type FormEvent } from "react";
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
import { Connectors } from "./Connectors";
import { Skills } from "./Skills";
import { StatusLine, useNote } from "./ui";

export const MARKETPLACE_NOTE =
  "Skills, plugins and connectors in one place. Fetching a list installs nothing, and every install still goes through the skill check and trust.";
export const PLUGINS_TIP =
  "A preview: plugins work, but how they install and trust may change.";
export const PLUGINS_MCP_NOTE = "MCP servers you add yourself are on the Connectors tab.";
export const INSTALL_HINT = "Nothing from the bundle runs. It installs off and untrusted.";
export const MARKET_HINT = "An https index URL. Fetch only lists entries; Install is a separate step.";
export const XAI_OFFICIAL_PRESET_ID = "xai-official";
export const XAI_OFFICIAL_MARKETPLACE_URL =
  "https://raw.githubusercontent.com/xai-org/plugin-marketplace/main/.grok-plugin/marketplace.json";
export const OFFICIAL_CATALOG_LABEL = "Load official xAI catalog";
export const OFFICIAL_CATALOG_HINT =
  "The official xAI catalog. Fetching it installs nothing. Install still goes through trust before enable.";
export const MARKET_EMPTY = "No marketplace entries.";
export const MARKET_NONE_MATCHED = "No entries match that search.";
export const REMOVE_HINT = "Remove the installed copy.";
export const UPDATE_HINT = "Update available. Remove yours above, then install this one.";
export const PLUGINS_NOTE =
  "Bundles stay off until you trust this version and turn them on. Trust covers the hook commands, MCP server commands, skills and agents a bundle lists. Fetching a marketplace list installs nothing.";
export const PLUGINS_EMPTY =
  "No plugins yet. Install a bundle above. It stays off until you trust it and turn it on.";

type Tab = "plugins" | "skills" | "connectors";
const TABS: { id: Tab; label: string }[] = [
  { id: "plugins", label: "Plugins" },
  { id: "skills", label: "Skills" },
  { id: "connectors", label: "Connectors" },
];

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

/** -1, 0 or 1 for dotted versions, numbers compared as numbers ("1.10" > "1.9").
 * A part that isn't a number compares as text. */
export function compareVersions(a: string, b: string): number {
  const left = a.trim().replace(/^v/i, "").split(/[.+-]/);
  const right = b.trim().replace(/^v/i, "").split(/[.+-]/);
  for (let i = 0; i < Math.max(left.length, right.length); i++) {
    const x = left[i] ?? "0";
    const y = right[i] ?? "0";
    const nx = Number(x);
    const ny = Number(y);
    const diff = Number.isFinite(nx) && Number.isFinite(ny) ? nx - ny : x.localeCompare(y);
    if (diff !== 0) return diff < 0 ? -1 : 1;
  }
  return 0;
}

/** What an index entry is to this computer: not here, installed, or newer than the copy here. */
export function entryStatus(entry: PluginEntry, rows: PluginRow[]): "new" | "installed" | "update" {
  const here = rows.find((row) => row.name === entry.name);
  if (!here) return "new";
  const indexSha = (entry.sha ?? "").trim().toLowerCase();
  const hereSha = (here.installedSha ?? "").trim().toLowerCase();
  if (/^[0-9a-f]{40}$/.test(indexSha) && /^[0-9a-f]{40}$/.test(hereSha)) {
    return indexSha === hereSha ? "installed" : "update";
  }
  return compareVersions(entry.version, here.version) > 0 ? "update" : "installed";
}

/** Entries whose name, description, author, category or keywords hold `query`, in `category` if one is picked. */
export function filterEntries(entries: PluginEntry[], query: string, category: string): PluginEntry[] {
  const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  return entries.filter((entry) => {
    if (category && entry.category !== category) return false;
    const text = [entry.name, entry.description, entry.author ?? "", entry.category ?? "", ...(entry.keywords ?? [])]
      .join(" ")
      .toLowerCase();
    return words.every((word) => text.includes(word));
  });
}

/** The Marketplace page: Plugins, Skills and Connectors tabs. Skills and
 * Connectors are the same lists as their own pages. */
export function Marketplace({
  onUse,
  remote = false,
}: {
  /** A skill's Use button: the chat runs it. */
  onUse: (text: string) => void;
  remote?: boolean;
}) {
  const [tab, setTab] = useState<Tab>("plugins");
  return (
    <section className="labs marketplace" aria-label="Marketplace">
      <h2>Marketplace</h2>
      <p className="note">{MARKETPLACE_NOTE}</p>
      <div className="code-side-tabs marketplace-tabs" role="tablist" aria-label="Marketplace kinds">
        {TABS.map((t) => (
          <button
            key={t.id}
            type="button"
            role="tab"
            id={`marketplace-tab-${t.id}`}
            aria-selected={tab === t.id}
            aria-controls={`marketplace-panel-${t.id}`}
            tabIndex={tab === t.id ? 0 : -1}
            onClick={() => setTab(t.id)}
            onKeyDown={(e) => {
              if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
              e.preventDefault();
              const at = TABS.findIndex((x) => x.id === tab);
              const step = e.key === "ArrowLeft" ? -1 : 1;
              const to = TABS[(at + step + TABS.length) % TABS.length].id;
              setTab(to);
              document.getElementById(`marketplace-tab-${to}`)?.focus();
            }}
          >
            {t.label}
          </button>
        ))}
      </div>
      <div role="tabpanel" id={`marketplace-panel-${tab}`} aria-labelledby={`marketplace-tab-${tab}`}>
        {tab === "plugins" && <PluginsTab onOpenConnectors={() => setTab("connectors")} />}
        {tab === "skills" && <Skills onUse={onUse} onOpenConnectors={() => setTab("connectors")} remote={remote} />}
        {tab === "connectors" && <Connectors remote={remote} />}
      </div>
    </section>
  );
}

/** Marketplace → Plugins: the bundles with their trust and on/off state, and
 * an index to browse. Moved here from Settings → Labs. */
export function PluginsTab({ onOpenConnectors }: { onOpenConnectors?: () => void } = {}) {
  const [page, setPage] = useState<PluginPage | null>(null);
  const [source, setSource] = useState("");
  const [marketUrl, setMarketUrl] = useState("");
  const [entries, setEntries] = useState<PluginEntry[]>([]);
  /** A fetch came back, so an empty list means the index has none. */
  const [fetched, setFetched] = useState(false);
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState("");
  const { note, ok, fail } = useNote();
  const [busy, setBusy] = useState(false);

  // Runs each time the tab shows; a market address typed here and not used stays.
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

  const categories = useMemo(
    () => [...new Set(entries.map((e) => e.category ?? "").filter(Boolean))].sort((a, b) => a.localeCompare(b)),
    [entries],
  );
  const shown = filterEntries(entries, query, category);

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

  async function install(event: { preventDefault(): void }, from: string, sha?: string) {
    event.preventDefault();
    if (!from.trim() || busy) return;
    const done = await run(() => (sha ? installPlugin(from, sha) : installPlugin(from)));
    if (done?.note.startsWith("Installed")) setSource("");
  }

  async function fetchList(event: FormEvent, url = marketUrl) {
    event.preventDefault();
    if (busy) return;
    const done = await run(async () => {
      const market = await fetchPluginMarket(url);
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
    setCategory("");
  }

  return (
    <div className="marketplace-plugins" role="group" aria-label="Plugins">
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
            tab.
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
      {page && page.rows.length === 0 && <p className="empty">{PLUGINS_EMPTY}</p>}
      {page && page.rows.length > 0 && (
        <ul className="rows" aria-label="Installed plugins">
          {page.rows.map((row) => (
            <li key={row.name} className="card" aria-label={row.name}>
              <div className="card-status">{state(row)}</div>
              <div className="card-title">
                {row.name} {row.version}
              </div>
              {row.description && <p className="card-detail-line plain plugin-desc">{row.description}</p>}
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
                <button type="button" disabled={busy} onClick={() => void run(() => pluginAct(row.name, next(row).act))}>
                  {next(row).label}
                </button>
                {row.installed && (
                  <button
                    type="button"
                    className="quiet"
                    disabled={busy}
                    onClick={() => void run(() => pluginAct(row.name, "remove"))}
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
          <input value={marketUrl} placeholder="https://… index URL" onChange={(e) => setMarketUrl(e.target.value)} />
        </label>
        <button type="submit" disabled={busy}>
          Fetch list
        </button>
        <button
          type="button"
          disabled={busy}
          onClick={(e) => {
            setMarketUrl(XAI_OFFICIAL_MARKETPLACE_URL);
            void fetchList(e, XAI_OFFICIAL_PRESET_ID);
          }}
        >
          {OFFICIAL_CATALOG_LABEL}
        </button>
      </form>
      <p className="note">{MARKET_HINT}</p>
      <p className="note">{OFFICIAL_CATALOG_HINT}</p>
      {fetched && entries.length === 0 && <p className="empty">{MARKET_EMPTY}</p>}
      {entries.length > 0 && (
        <div className="labs-row" role="search" aria-label="Filter the index">
          <label>
            Search the index
            <input type="search" value={query} onChange={(e) => setQuery(e.target.value)} />
          </label>
          {categories.length > 0 && (
            <label>
              Category
              <select value={category} onChange={(e) => setCategory(e.target.value)}>
                <option value="">All categories</option>
                {categories.map((c) => (
                  <option key={c} value={c}>
                    {c}
                  </option>
                ))}
              </select>
            </label>
          )}
        </div>
      )}
      {entries.length > 0 && shown.length === 0 && <p className="empty">{MARKET_NONE_MATCHED}</p>}
      {shown.length > 0 && (
        <ul className="rows" aria-label="Marketplace">
          {shown.map((entry) => {
            const status = page ? entryStatus(entry, page.rows) : "new";
            return (
              <li key={entry.source} className="card" aria-label={entry.name}>
                <div className="card-title">
                  {entry.name} {entry.version}{" "}
                  {entry.category && <span className="badge">{entry.category}</span>}{" "}
                  {status === "installed" && <span className="badge">Installed</span>}
                  {status === "update" && <span className="badge warn">Update</span>}
                </div>
                {entry.author && <p className="card-detail-line plain">by {entry.author}</p>}
                {entry.description && <p className="card-detail-line">{entry.description}</p>}
                <code className="card-detail-line">{entry.source}</code>
                {status === "update" && <p className="note">{UPDATE_HINT}</p>}
                {status === "new" && (
                  <div className="card-buttons">
                    <button
                      type="button"
                      disabled={busy}
                      aria-label={`Install ${entry.name}`}
                      onClick={(e) => void install(e, entry.source, entry.sha)}
                    >
                      Install
                    </button>
                  </div>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}

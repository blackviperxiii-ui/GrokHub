import { useCallback, useEffect, useState } from "react";
import {
  fleetMachines,
  fleetUsage,
  getFleetRows,
  setFleetRow,
  type FleetMachine,
  type FleetRow,
  type FleetUsage,
  type UsageTokens,
} from "./api";
import { ago } from "./Devices";
import { StatusLine, useNote } from "./ui";

/** Machines re-read this often while the page shows, so a computer that went to sleep dims. */
export const MACHINES_MS = 60_000;
/** Usage re-reads this often while the page shows; Claude's transcripts take a moment. */
export const USAGE_MS = 5 * 60_000;

export const FLEET_NOTE =
  "Every computer running this app that joined this one (Devices → Join another computer), and the coding plans in use here.";
export const NO_OTHERS = "No other computer has joined yet. On another computer, open Devices → Join another computer.";
export const NO_CLAUDE = "Claude Code isn't on this computer.";
export const NO_GROK = "Shows after the next chat signed in with Grok.";
export const ROLLING_NOTE = "No 5-hour break in view, so this is the last 5 hours.";

const ROW_NAMES: Record<FleetRow["id"], string> = { cursor: "Cursor", antigravity: "Antigravity" };
const ROW_FROM: Record<FleetRow["id"], string> = {
  cursor: "From cursor.com/dashboard → Usage.",
  antigravity: "From Antigravity → Settings → Models, or /usage in its CLI.",
};

/** "1.2M", "340k", "950". */
export function tokens(n: number): string {
  const k = Math.round(n / 1_000);
  if (k >= 1_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${k}k`;
  return String(n);
}

/** "2.1M in · 340k out": what went to the model (cache included) and what came back. */
export function tokensLine(t: UsageTokens): string {
  return `${tokens(t.input + t.cacheWrite + t.cacheRead)} in · ${tokens(t.output)} out`;
}

/** "resets in 27 min", "resets in 1 h 10 min", "resets in 3 days", "reset". */
export function resetsIn(at: number, now: number): string {
  const minutes = Math.ceil((at - now) / 60_000);
  if (minutes <= 0) return "reset";
  if (minutes < 60) return `resets in ${minutes} min`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `resets in ${hours} h${minutes % 60 ? ` ${minutes % 60} min` : ""}`;
  const days = Math.round(hours / 24);
  return `resets in ${days} ${days === 1 ? "day" : "days"}`;
}

/** "CPU 23% · 9.0 of 16 GB used · 3.6.0". */
export function machineLine(m: FleetMachine): string {
  const cpu = m.cpuPct === null ? "CPU –" : `CPU ${m.cpuPct}%`;
  const gb = (mb: number) => (mb / 1024).toFixed(mb >= 10 * 1024 ? 0 : 1);
  const mem =
    m.ramMb === null ? "Memory –" : m.freeRamMb === null ? `${gb(m.ramMb)} GB` : `${gb(m.ramMb - m.freeRamMb)} of ${gb(m.ramMb)} GB used`;
  return `${cpu} · ${mem} · ${m.appVersion}`;
}

export function seenLine(m: FleetMachine, now: number): string {
  if (m.thisComputer) return "This computer";
  return m.online ? `Online · ${ago(m.atMs, now)}` : `Asleep · last seen ${ago(m.atMs, now)}`;
}

/** Part used, 0-1, from what's left of a limit; null when either is unknown. */
export function usedOf(left: number | null, limit: number | null): number | null {
  if (left === null || limit === null || limit <= 0) return null;
  return Math.min(1, Math.max(0, 1 - left / limit));
}

function Bar({ frac, label }: { frac: number; label: string }) {
  const pct = Math.round(Math.min(1, Math.max(0, frac)) * 100);
  return (
    <span
      className={`xp-track fleet-bar${pct >= 90 ? " fleet-bar--hot" : ""}`}
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={pct}
    >
      <span className="xp-fill" style={{ width: `${pct}%` }} />
    </span>
  );
}

function Machines({ machines, now }: { machines: FleetMachine[] | null; now: number }) {
  if (machines === null) return <p className="empty">Checking…</p>;
  return (
    <>
      <ul className="rows" aria-label="Machines">
        {machines.map((m) => (
          <li key={m.id} className={`card device fleet-machine${m.online ? "" : " fleet-machine--asleep"}`}>
            <div className="card-title">{m.name}</div>
            <div className="card-status">{seenLine(m, now)}</div>
            <div className="card-detail-line">{machineLine(m)}</div>
          </li>
        ))}
      </ul>
      {machines.length === 1 && <p className="empty">{NO_OTHERS}</p>}
    </>
  );
}

function Claude({ usage, now }: { usage: FleetUsage | null; now: number }) {
  const claude = usage?.claude ?? null;
  return (
    <div className="card device" aria-label="Claude">
      <div className="card-title">Claude</div>
      {usage === null && <div className="card-status">Checking…</div>}
      {usage !== null && !claude && <div className="card-status">{NO_CLAUDE}</div>}
      {claude && (
        <>
          <div className="card-status">
            {`5-hour window: ${tokensLine(claude.window)}`}
            {claude.windowResetsAtMs !== null && `, ${resetsIn(claude.windowResetsAtMs, now)}`}
          </div>
          {claude.windowRolling && <div className="card-detail-line">{ROLLING_NOTE}</div>}
          <div className="card-detail-line">{`Last 7 days: ${tokensLine(claude.week)}`}</div>
          {claude.lastAtMs !== null && <div className="card-detail-line">{`Last used ${ago(claude.lastAtMs, now)}`}</div>}
        </>
      )}
    </div>
  );
}

function Grok({ usage, now }: { usage: FleetUsage | null; now: number }) {
  const grok = usage?.grok ?? null;
  const requests = grok ? usedOf(grok.requestsLeft, grok.requestsLimit) : null;
  const pool = grok ? usedOf(grok.tokensLeft, grok.tokensLimit) : null;
  return (
    <div className="card device" aria-label="Grok">
      <div className="card-title">Grok</div>
      {usage === null && <div className="card-status">Checking…</div>}
      {usage !== null && !grok && <div className="card-status">{NO_GROK}</div>}
      {grok && (
        <>
          <div className="card-status">
            {grok.resetsAtMs !== null ? resetsIn(grok.resetsAtMs, now) : `As of ${ago(grok.seenMs, now)}`}
          </div>
          {requests !== null && (
            <div className="card-detail-line">
              {`Requests: ${grok.requestsLeft} of ${grok.requestsLimit} left`}
              <Bar frac={requests} label="Grok requests used" />
            </div>
          )}
          {pool !== null && (
            <div className="card-detail-line">
              {`Tokens: ${tokens(grok.tokensLeft ?? 0)} of ${tokens(grok.tokensLimit ?? 0)} left`}
              <Bar frac={pool} label="Grok tokens used" />
            </div>
          )}
        </>
      )}
    </div>
  );
}

/** `yyyy-mm-dd` for a date input, in local time. */
function dayValue(at: number | null): string {
  if (at === null) return "";
  const d = new Date(at);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/** Local midnight of a date input's value; null when empty. */
function dayMs(value: string): number | null {
  if (!value) return null;
  const [y, m, d] = value.split("-").map(Number);
  return new Date(y, m - 1, d).getTime();
}

/** A percent field: empty clears it, whole numbers over 100 are kept as 100,
 * and anything else is refused (undefined) so a typo never erases a saved value. */
export function pctValue(text: string): number | null | undefined {
  const t = text.trim().replace(/%$/, "").trim();
  if (!t) return null;
  if (!/^\d+$/.test(t)) return undefined;
  return Math.min(100, Number(t));
}

export const PCT_ONLY = "Percent used is a whole number from 0 to 100, or empty.";

function ManualCard({ row, now, onSave }: { row: FleetRow; now: number; onSave: (row: FleetRow) => Promise<boolean> }) {
  const name = ROW_NAMES[row.id];
  const [editing, setEditing] = useState(false);
  const [windowPct, setWindowPct] = useState("");
  const [periodPct, setPeriodPct] = useState("");
  const [periodDay, setPeriodDay] = useState("");
  const [bad, setBad] = useState(false);

  function edit() {
    setWindowPct(row.windowPct === null ? "" : String(row.windowPct));
    setPeriodPct(row.periodPct === null ? "" : String(row.periodPct));
    setPeriodDay(dayValue(row.periodResetsAtMs));
    setBad(false);
    setEditing(true);
  }

  async function save() {
    const shortPct = pctValue(windowPct);
    const longPct = pctValue(periodPct);
    if (shortPct === undefined || longPct === undefined) {
      setBad(true);
      return;
    }
    setBad(false);
    // Stay open on a failed save, so what was typed is still there to try again.
    if (await onSave({ ...row, windowPct: shortPct, periodPct: longPct, periodResetsAtMs: dayMs(periodDay) })) setEditing(false);
  }

  const filled = row.updatedAt > 0;
  return (
    <div className="card device" aria-label={name}>
      <div className="card-title">{name}</div>
      <div className="card-status">{filled ? `You filled this in ${ago(row.updatedAt, now)}.` : "Not filled in yet."}</div>
      {!editing && row.windowPct !== null && (
        <div className="card-detail-line">
          {`Short window: ${row.windowPct}% used`}
          <Bar frac={row.windowPct / 100} label={`${name} short window used`} />
        </div>
      )}
      {!editing && row.periodPct !== null && (
        <div className="card-detail-line">
          {`This period: ${row.periodPct}% used`}
          {row.periodResetsAtMs !== null && `, ${resetsIn(row.periodResetsAtMs, now)}`}
          <Bar frac={row.periodPct / 100} label={`${name} period used`} />
        </div>
      )}
      {editing && (
        <>
          <label>
            Short window % used
            <input inputMode="numeric" value={windowPct} onChange={(e) => setWindowPct(e.target.value)} />
          </label>
          <label>
            This period % used
            <input inputMode="numeric" value={periodPct} onChange={(e) => setPeriodPct(e.target.value)} />
          </label>
          <label>
            Period resets on
            <input type="date" value={periodDay} onChange={(e) => setPeriodDay(e.target.value)} />
          </label>
          {bad && <p className="status status--error">{PCT_ONLY}</p>}
        </>
      )}
      <div className="card-detail-line">{ROW_FROM[row.id]}</div>
      <div className="card-buttons">
        {editing ? (
          <>
            <button type="button" onClick={() => void save()}>
              Save
            </button>
            <button type="button" className="quiet" onClick={() => setEditing(false)}>
              Cancel
            </button>
          </>
        ) : (
          <button type="button" className="quiet" onClick={edit}>
            {filled ? "Update" : "Fill in"}
          </button>
        )}
      </div>
    </div>
  );
}

/** The Fleet page: the computers that joined this one, then each coding plan's usage. */
export function Fleet({ shown = true }: { shown?: boolean }) {
  const [machines, setMachines] = useState<FleetMachine[] | null>(null);
  const [usage, setUsage] = useState<FleetUsage | null>(null);
  const [rows, setRows] = useState<FleetRow[] | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const [busy, setBusy] = useState(false);
  const { note, fail, clear } = useNote();

  const readMachines = useCallback(
    () =>
      fleetMachines().then((next) => {
        setMachines(next);
        setNow(Date.now());
      }, fail),
    [fail],
  );
  const readUsage = useCallback(
    () =>
      Promise.all([fleetUsage(), getFleetRows()]).then(([nextUsage, nextRows]) => {
        setUsage(nextUsage);
        setRows(nextRows);
        setNow(Date.now());
      }, fail),
    [fail],
  );

  useEffect(() => {
    if (!shown) return;
    void readMachines();
    void readUsage();
    const machinesTimer = window.setInterval(() => void readMachines(), MACHINES_MS);
    const usageTimer = window.setInterval(() => void readUsage(), USAGE_MS);
    return () => {
      window.clearInterval(machinesTimer);
      window.clearInterval(usageTimer);
    };
  }, [shown, readMachines, readUsage]);

  async function refresh() {
    setBusy(true);
    clear();
    try {
      await Promise.all([readMachines(), readUsage()]);
    } finally {
      setBusy(false);
    }
  }

  async function save(row: FleetRow): Promise<boolean> {
    try {
      setRows(await setFleetRow(row));
      setNow(Date.now());
      clear();
      return true;
    } catch (err) {
      fail(err);
      return false;
    }
  }

  return (
    <section className="devices fleet" aria-label="Fleet">
      <p className="note">{FLEET_NOTE}</p>
      <div className="card-buttons">
        <button type="button" className="fleet-refresh" disabled={busy} onClick={() => void refresh()}>
          {busy ? "Refreshing…" : "Refresh"}
        </button>
      </div>
      <StatusLine note={note} />
      <h2>Machines</h2>
      <Machines machines={machines} now={now} />
      <h2>Coding plans</h2>
      <Claude usage={usage} now={now} />
      <Grok usage={usage} now={now} />
      {rows?.map((row) => <ManualCard key={row.id} row={row} now={now} onSave={save} />)}
    </section>
  );
}

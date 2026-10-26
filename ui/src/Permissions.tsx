import { useEffect, useState, type FormEvent } from "react";
import {
  addPermissionRule,
  forgetGrant,
  getPermissions,
  rememberGrant,
  removePermissionRule,
  type PermissionPage,
  type RuleKind,
} from "./api";

export const RULES_NOTE =
  "Deny wins over ask, and ask wins over allow. Dangerous commands still ask, including after Allow always.";
export const RULE_HINT =
  "Bash(git *), Read(src/**), Edit(src/**), WebFetch(domain:example.com), MCPTool(server__*)";
export const GRANTS_NOTE = "Allow always for this project. Dangerous commands are not stored.";

const KINDS: { value: RuleKind; label: string }[] = [
  { value: "allow", label: "Allow" },
  { value: "ask", label: "Ask" },
  { value: "deny", label: "Deny" },
];

const LABEL: Record<RuleKind, string> = { allow: "Allow", ask: "Ask", deny: "Deny" };

/** Settings → Permissions: command rules for every project, and this
 * project's remembered grants, from the same files the gate reads. */
export function Permissions() {
  const [page, setPage] = useState<PermissionPage | null>(null);
  const [rule, setRule] = useState("");
  const [kind, setKind] = useState<RuleKind>("allow");
  const [grant, setGrant] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    getPermissions().then(setPage, (err) => setNote(String(err)));
  }, []);

  async function run(work: () => Promise<PermissionPage>, done: string) {
    setBusy(true);
    try {
      setPage(await work());
      setNote(done);
      return true;
    } catch (err) {
      setNote(String(err));
      // The cabin may have changed the list; show what the files hold now.
      await getPermissions().then(setPage, () => {});
      return false;
    } finally {
      setBusy(false);
    }
  }

  async function addRule(event: FormEvent) {
    event.preventDefault();
    if (!rule.trim() || busy) return;
    if (await run(() => addPermissionRule(rule, kind), `Saved: ${LABEL[kind]} ${rule.trim()}.`)) setRule("");
  }

  async function remember(event: FormEvent) {
    event.preventDefault();
    if (!grant.trim() || busy) return;
    if (await run(() => rememberGrant(grant), `Remembered ${grant.trim()} for this project.`)) setGrant("");
  }

  return (
    <div className="permissions" role="group" aria-label="Permissions">
      <h2>Command rules</h2>
      <p className="note">{RULES_NOTE}</p>
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
      <form className="labs-row" onSubmit={(e) => void addRule(e)}>
        <label>
          Rule
          <input value={rule} placeholder={RULE_HINT} onChange={(e) => setRule(e.target.value)} />
        </label>
        <div className="chip-group" role="group" aria-label="Rule action">
          {KINDS.map((k) => (
            <button
              key={k.value}
              type="button"
              className="chip"
              aria-pressed={kind === k.value}
              onClick={() => setKind(k.value)}
            >
              {k.label}
            </button>
          ))}
        </div>
        <button type="submit" disabled={busy || !rule.trim()}>
          Add rule
        </button>
      </form>
      <p className="note">Saved for every project.</p>
      {page && page.rules.length === 0 && <p className="empty">No saved rules.</p>}
      {page && page.rules.length > 0 && (
        <ul className="rows" aria-label="Saved rules">
          {page.rules.map((r, i) => (
            <li key={`${i}-${r.rule}`} className="card" aria-label={`${LABEL[r.action]} ${r.rule}`}>
              <div className="card-title">
                <span className={`badge rule-${r.action}`}>{LABEL[r.action]}</span> <code>{r.rule}</code>
              </div>
              <div className="card-buttons">
                <button
                  type="button"
                  className="quiet"
                  disabled={busy}
                  onClick={() => void run(() => removePermissionRule(i, r.rule), `Removed ${r.rule}.`)}
                >
                  Remove
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}
      {page && page.imported.length > 0 && (
        <>
          <p className="note">From .claude/settings.json. Read only.</p>
          <ul className="rows" aria-label="Project rules">
            {page.imported.map((r, i) => (
              <li key={`${i}-${r.rule}`} className="card" aria-label={`${LABEL[r.action]} ${r.rule}`}>
                <div className="card-title">
                  <span className={`badge rule-${r.action}`}>{LABEL[r.action]}</span> <code>{r.rule}</code>
                </div>
              </li>
            ))}
          </ul>
        </>
      )}
      <h2>Remembered commands</h2>
      <p className="note">{GRANTS_NOTE}</p>
      <form className="labs-row" onSubmit={(e) => void remember(e)}>
        <label>
          Command
          <input value={grant} placeholder="cargo build" onChange={(e) => setGrant(e.target.value)} />
        </label>
        <button type="submit" disabled={busy || !grant.trim()}>
          Remember
        </button>
      </form>
      {page && page.grants.length === 0 && <p className="empty">No remembered commands for this project.</p>}
      {page && page.grants.length > 0 && (
        <ul className="rows" aria-label="Remembered commands">
          {page.grants.map((g) => (
            <li key={g} className="card" aria-label={g}>
              <div className="card-title">
                <code>{g}</code>
              </div>
              <div className="card-buttons">
                <button
                  type="button"
                  className="quiet"
                  disabled={busy}
                  onClick={() => void run(() => forgetGrant(g), `Forgot ${g}.`)}
                >
                  Remove
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

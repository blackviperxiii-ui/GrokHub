import { useEffect, useState, type FormEvent } from "react";
import {
  clearAgentTasks,
  getAgentTasks,
  getTeam,
  removeAgent,
  removeTeam,
  saveAgent,
  saveTeam,
  type AgentAccess,
  type AgentKind,
  type AgentProfile,
  type AgentTask,
  type AgentTaskState,
  type AgentTeam,
  type TeamPage,
} from "./teamApi";
import { ConfirmRow, PageHead, StatusLine, useNote } from "./ui";

export const KINDS: { kind: AgentKind; label: string; hint: string }[] = [
  { kind: "role", label: "Role", hint: "A job to hand off, like research or writing." },
  { kind: "family", label: "Family", hint: "Someone at home, with their own chats and memory." },
  { kind: "staff", label: "Staff", hint: "Someone who works with you." },
  {
    kind: "customer",
    label: "Customer",
    hint: "Answers questions, read only. It never starts a message.",
  },
];

export const ACCESS_LABELS: Record<AgentAccess, string> = {
  read_only: "Read only",
  ask: "Ask first",
  auto: "Auto",
};

export const STATE_LABELS: Record<AgentTaskState, string> = {
  TASK_STATE_SUBMITTED: "Sent",
  TASK_STATE_WORKING: "Working",
  TASK_STATE_INPUT_REQUIRED: "Needs an answer",
  TASK_STATE_AUTH_REQUIRED: "Needs a sign-in",
  TASK_STATE_COMPLETED: "Answered",
  TASK_STATE_FAILED: "Failed",
  TASK_STATE_CANCELED: "Stopped",
  TASK_STATE_REJECTED: "Refused",
};

export const TEAM_LEDE = "Agents you set up, and what they were asked.";
export const AGENTS_EMPTY = "No agents yet. Make one from a kind above.";
export const TASKS_EMPTY =
  "No messages yet. When you or one of your agents asks an agent something, it shows here.";
export const KEPT_FOLDER =
  "Its chats and memory stay in its folder and come back if you make an agent with the same name.";

const POLL_MS = 5000;
const kindLabel = (kind: AgentKind) => KINDS.find((k) => k.kind === kind)?.label ?? kind;

/** A file id from a name: lowercase letters, digits and dashes, at most 40
 * long, made unique against `taken`. */
export function idFor(name: string, taken: string[]): string {
  const base =
    name
      .normalize("NFKD")
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+/, "")
      .slice(0, 36)
      .replace(/-+$/, "") || "agent";
  let id = base;
  for (let n = 2; taken.includes(id); n++) id = `${base}-${n}`;
  return id;
}

function newProfile(kind: AgentKind): AgentProfile {
  return {
    id: "",
    name: "",
    kind,
    access: kind === "customer" ? "read_only" : "ask",
    about: "",
    desktop: false,
    share: false,
  };
}

type AgentDraft = { profile: AgentProfile; add: boolean };
type TeamDraft = { team: AgentTeam; add: boolean };
type Confirm = { kind: "agent" | "team"; id: string } | "log" | null;

/** The person's agents and teams, and the log of what each was asked.
 * `shown` keeps the log fresh only while the page is on screen. */
export function Team({ shown = true }: { shown?: boolean }) {
  const [page, setPage] = useState<TeamPage | null>(null);
  const [tasks, setTasks] = useState<AgentTask[] | null>(null);
  const [agent, setAgent] = useState<AgentDraft | null>(null);
  const [team, setTeam] = useState<TeamDraft | null>(null);
  const [confirm, setConfirm] = useState<Confirm>(null);
  const [busy, setBusy] = useState(false);
  const { note, ok, fail } = useNote();

  useEffect(() => {
    getTeam().then(setPage, fail);
  }, [fail]);

  useEffect(() => {
    if (!shown) return;
    let live = true;
    const load = () =>
      getAgentTasks().then(
        (now) => live && setTasks(now),
        (err: unknown) => {
          if (!live) return;
          setTasks([]);
          fail(err);
        },
      );
    void load();
    const timer = window.setInterval(() => void load(), POLL_MS);
    return () => {
      live = false;
      window.clearInterval(timer);
    };
  }, [shown, fail]);

  async function run(work: () => Promise<TeamPage>, done: string) {
    setBusy(true);
    try {
      setPage(await work());
      ok(done);
      setConfirm(null);
      return true;
    } catch (err) {
      fail(err);
      return false;
    } finally {
      setBusy(false);
    }
  }

  const name = (id: string) => page?.agents.find((a) => a.id === id)?.name ?? id;

  async function submitAgent(profile: AgentProfile, add: boolean) {
    const taken = page?.agents.map((a) => a.id) ?? [];
    const named = { ...profile, name: profile.name.trim() };
    const saved = add ? { ...named, id: idFor(named.name, taken) } : named;
    const done = add ? `Made ${saved.name}.` : `Saved ${saved.name}.`;
    if (await run(() => saveAgent(saved, add), done)) setAgent(null);
  }

  async function submitTeam(draft: AgentTeam, add: boolean) {
    const taken = page?.teams.map((t) => t.id) ?? [];
    const named = { ...draft, name: draft.name.trim() };
    const saved = add ? { ...named, id: idFor(named.name, taken) } : named;
    if (await run(() => saveTeam(saved, add), `Saved ${saved.name}.`)) setTeam(null);
  }

  async function clearLog() {
    setBusy(true);
    try {
      await clearAgentTasks();
      setTasks([]);
      setConfirm(null);
      ok("Cleared the task log.");
    } catch (err) {
      fail(err);
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="team" aria-label="Team">
      <PageHead title="Team">{TEAM_LEDE}</PageHead>
      <StatusLine note={note} />
      {page?.problem && (
        <p className="status status--error" role="alert">
          The team file breaks a rule: {page.problem}
        </p>
      )}
      {page === null && !note && <p className="empty">Loading your agents…</p>}
      {page && (
        <>
          <h2>Agents</h2>
          <div className="team-new" role="group" aria-label="New agent">
            {KINDS.map((k) => (
              <button
                key={k.kind}
                type="button"
                title={k.hint}
                disabled={busy}
                onClick={() => setAgent({ profile: newProfile(k.kind), add: true })}
              >
                New {k.label.toLowerCase()}
              </button>
            ))}
          </div>
          {agent?.add && (
            <AgentForm
              draft={agent}
              page={page}
              busy={busy}
              onSave={(p) => void submitAgent(p, true)}
              onCancel={() => setAgent(null)}
            />
          )}
          {page.agents.length === 0 && <p className="empty">{AGENTS_EMPTY}</p>}
          {page.agents.length > 0 && (
            <ul className="rows">
              {page.agents.map((a) =>
                agent && !agent.add && agent.profile.id === a.id ? (
                  <li key={a.id}>
                    <AgentForm
                      draft={agent}
                      page={page}
                      busy={busy}
                      onSave={(p) => void submitAgent(p, false)}
                      onCancel={() => setAgent(null)}
                    />
                  </li>
                ) : (
                  <li key={a.id} className="card team-agent" aria-label={a.name}>
                    <div className="card-status">
                      {kindLabel(a.kind)} · {ACCESS_LABELS[a.access]}
                      {a.team && ` · ${page.teams.find((t) => t.id === a.team)?.name ?? a.team}`}
                      {a.desktop && " · desktop"}
                    </div>
                    <div className="card-title">{a.name}</div>
                    {a.about && <p className="team-about">{a.about.split("\n")[0]}</p>}
                    {(a.talks_to?.length ?? 0) > 0 && (
                      <p className="team-about">Talks to {a.talks_to?.map(name).join(", ")}</p>
                    )}
                    {confirm !== "log" && confirm?.kind === "agent" && confirm.id === a.id ? (
                      <ConfirmRow
                        question={`Remove ${a.name}?`}
                        detail={KEPT_FOLDER}
                        action="Remove"
                        busy={busy}
                        onConfirm={() =>
                          void run(() => removeAgent(a.id), `Removed ${a.name}.`)
                        }
                        onCancel={() => setConfirm(null)}
                      />
                    ) : (
                      <div className="card-buttons">
                        <button
                          type="button"
                          disabled={busy}
                          onClick={() => setAgent({ profile: a, add: false })}
                        >
                          Edit
                        </button>
                        <button
                          type="button"
                          className="quiet"
                          disabled={busy}
                          onClick={() => setConfirm({ kind: "agent", id: a.id })}
                        >
                          Remove
                        </button>
                      </div>
                    )}
                  </li>
                ),
              )}
            </ul>
          )}

          <h2>Teams</h2>
          <p className="team-about">
            On a team, the lead and each member may message each other.
          </p>
          {!team?.add && (
            <div className="card-buttons">
              <button
                type="button"
                disabled={busy || page.agents.length === 0}
                title={page.agents.length === 0 ? "Make an agent to lead it first" : undefined}
                onClick={() =>
                  setTeam({ team: { id: "", name: "", lead: page.agents[0]?.id ?? "" }, add: true })
                }
              >
                New team
              </button>
            </div>
          )}
          {team?.add && (
            <TeamForm
              draft={team}
              page={page}
              busy={busy}
              onSave={(t) => void submitTeam(t, true)}
              onCancel={() => setTeam(null)}
            />
          )}
          {page.teams.length > 0 && (
            <ul className="rows">
              {page.teams.map((t) =>
                team && !team.add && team.team.id === t.id ? (
                  <li key={t.id}>
                    <TeamForm
                      draft={team}
                      page={page}
                      busy={busy}
                      onSave={(next) => void submitTeam(next, false)}
                      onCancel={() => setTeam(null)}
                    />
                  </li>
                ) : (
                  <li key={t.id} className="card team-team" aria-label={t.name}>
                    <div className="card-status">
                      Led by {name(t.lead)} ·{" "}
                      {page.agents.filter((a) => a.team === t.id).length} on it
                    </div>
                    <div className="card-title">{t.name}</div>
                    {confirm !== "log" && confirm?.kind === "team" && confirm.id === t.id ? (
                      <ConfirmRow
                        question={`Remove ${t.name}?`}
                        detail="Its agents stay, on no team."
                        action="Remove"
                        busy={busy}
                        onConfirm={() => void run(() => removeTeam(t.id), `Removed ${t.name}.`)}
                        onCancel={() => setConfirm(null)}
                      />
                    ) : (
                      <div className="card-buttons">
                        <button
                          type="button"
                          disabled={busy}
                          onClick={() => setTeam({ team: t, add: false })}
                        >
                          Edit
                        </button>
                        <button
                          type="button"
                          className="quiet"
                          disabled={busy}
                          onClick={() => setConfirm({ kind: "team", id: t.id })}
                        >
                          Remove
                        </button>
                      </div>
                    )}
                  </li>
                ),
              )}
            </ul>
          )}
        </>
      )}

      <h2>Task log</h2>
      {tasks === null && <p className="empty">Loading the task log…</p>}
      {tasks?.length === 0 && <p className="empty">{TASKS_EMPTY}</p>}
      {tasks && tasks.length > 0 && (
        <>
          {confirm === "log" ? (
            <ConfirmRow
              question="Clear the task log?"
              action="Clear"
              busy={busy}
              onConfirm={() => void clearLog()}
              onCancel={() => setConfirm(null)}
            />
          ) : (
            <div className="card-buttons">
              <button
                type="button"
                className="quiet"
                disabled={busy}
                onClick={() => setConfirm("log")}
              >
                Clear log
              </button>
            </div>
          )}
          <ul className="rows" aria-label="Task log">
            {tasks.map((t) => {
              const who = t.from === "person" ? "You" : name(t.from.agent);
              return (
                <li key={t.id} className="card team-task" aria-label={`${who} asked ${name(t.to)}`}>
                  <div className="card-status">
                    {who} asked {name(t.to)} · {STATE_LABELS[t.state]} ·{" "}
                    {new Date(t.ended_ms).toLocaleString(undefined, {
                      month: "short",
                      day: "numeric",
                      hour: "numeric",
                      minute: "2-digit",
                    })}
                  </div>
                  <div className="card-title">{t.asked}</div>
                  {t.reply && (
                    <details>
                      <summary>{t.state === "TASK_STATE_COMPLETED" ? "Answer" : "Why"}</summary>
                      <p className="team-reply">{t.reply}</p>
                    </details>
                  )}
                </li>
              );
            })}
          </ul>
        </>
      )}
    </section>
  );
}

function AgentForm({
  draft,
  page,
  busy,
  onSave,
  onCancel,
}: {
  draft: AgentDraft;
  page: TeamPage;
  busy: boolean;
  onSave: (profile: AgentProfile) => void;
  onCancel: () => void;
}) {
  const [p, setP] = useState(draft.profile);
  const set = (patch: Partial<AgentProfile>) => setP((now) => ({ ...now, ...patch }));
  const customer = p.kind === "customer";
  const others = page.agents.filter((a) => a.id !== p.id);
  const talks = p.talks_to ?? [];
  const label = kindLabel(p.kind);

  function submit(event: FormEvent) {
    event.preventDefault();
    if (!p.name.trim() || busy) return;
    onSave(p);
  }

  return (
    <form
      className="card team-form"
      aria-label={draft.add ? `New ${label.toLowerCase()}` : `Edit ${draft.profile.name}`}
      onSubmit={submit}
    >
      <div className="card-status">{label}</div>
      <label>
        Name
        <input value={p.name} maxLength={60} onChange={(e) => set({ name: e.target.value })} />
      </label>
      <label>
        How it works and what it knows
        <textarea
          value={p.about}
          rows={3}
          maxLength={4000}
          onChange={(e) => set({ about: e.target.value })}
        />
      </label>
      <label>
        Access
        <select
          value={customer ? "read_only" : p.access}
          disabled={customer}
          onChange={(e) => set({ access: e.target.value as AgentAccess })}
        >
          {(Object.keys(ACCESS_LABELS) as AgentAccess[]).map((a) => (
            <option key={a} value={a}>
              {ACCESS_LABELS[a]}
            </option>
          ))}
        </select>
      </label>
      {!customer && (
        <label className="team-check">
          <input
            type="checkbox"
            checked={p.desktop}
            onChange={(e) => set({ desktop: e.target.checked })}
          />
          May use this computer's desktop when you allow it
        </label>
      )}
      {!customer && others.length > 0 && (
        <fieldset>
          <legend>Talks to</legend>
          {others.map((a) => (
            <label key={a.id} className="team-check">
              <input
                type="checkbox"
                checked={talks.includes(a.id)}
                onChange={(e) =>
                  set({
                    talks_to: e.target.checked
                      ? [...talks, a.id]
                      : talks.filter((id) => id !== a.id),
                  })
                }
              />
              {a.name}
            </label>
          ))}
        </fieldset>
      )}
      {page.teams.length > 0 && (
        <label>
          Team
          <select value={p.team ?? ""} onChange={(e) => set({ team: e.target.value })}>
            <option value="">No team</option>
            {page.teams.map((t) => (
              <option key={t.id} value={t.id}>
                {t.name}
              </option>
            ))}
          </select>
        </label>
      )}
      {page.tiers.length > 0 && (
        <label>
          Models
          <select value={p.model_tier ?? ""} onChange={(e) => set({ model_tier: e.target.value })}>
            <option value="">Any model</option>
            {page.tiers.map((t) => (
              <option key={t} value={t}>
                {t}
              </option>
            ))}
          </select>
        </label>
      )}
      <div className="card-buttons">
        <button type="submit" disabled={busy || !p.name.trim()}>
          {draft.add ? `Make ${label.toLowerCase()}` : "Save"}
        </button>
        <button type="button" className="quiet" onClick={onCancel}>
          Cancel
        </button>
      </div>
    </form>
  );
}

function TeamForm({
  draft,
  page,
  busy,
  onSave,
  onCancel,
}: {
  draft: TeamDraft;
  page: TeamPage;
  busy: boolean;
  onSave: (team: AgentTeam) => void;
  onCancel: () => void;
}) {
  const [t, setT] = useState(draft.team);

  function submit(event: FormEvent) {
    event.preventDefault();
    if (!t.name.trim() || !t.lead || busy) return;
    onSave(t);
  }

  return (
    <form
      className="card team-form"
      aria-label={draft.add ? "New team" : `Edit ${draft.team.name}`}
      onSubmit={submit}
    >
      <label>
        Name
        <input
          value={t.name}
          maxLength={60}
          onChange={(e) => setT((now) => ({ ...now, name: e.target.value }))}
        />
      </label>
      <label>
        Lead
        <select
          value={t.lead}
          onChange={(e) => setT((now) => ({ ...now, lead: e.target.value }))}
        >
          {page.agents.map((a) => (
            <option key={a.id} value={a.id}>
              {a.name}
            </option>
          ))}
        </select>
      </label>
      <div className="card-buttons">
        <button type="submit" disabled={busy || !t.name.trim() || !t.lead}>
          {draft.add ? "Make team" : "Save"}
        </button>
        <button type="button" className="quiet" onClick={onCancel}>
          Cancel
        </button>
      </div>
    </form>
  );
}

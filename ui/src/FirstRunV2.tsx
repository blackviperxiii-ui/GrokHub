// First run v2: after Auto Setup and Get to know you. Layout pick, connect
// suggested apps (MCP Connectors), skills, local-model offer, IDE import,
// sandbox confirmation (never turns it off), first task, and a short tour.
import { useCallback, useEffect, useRef, useState } from "react";
import {
  addCatalogConnector,
  connectorCatalog,
  getLocalAi,
  getSkills,
  setSkillOn,
  signInConnector,
  type CatalogRow,
  type LocalAiView,
  type SkillRow,
} from "./api";
import { PRODUCT_NAME } from "./brand";
import {
  finishFirstRunV2,
  FIRST_RUN_V2_EVENT,
  FIRST_RUN_V2_LABELS,
  FIRST_RUN_V2_ORDER,
  FIRST_TASKS,
  layoutBlurb,
  loadFirstRunV2,
  nextStep,
  prevStep,
  reopenFirstRunV2,
  saveFirstRunV2,
  SUGGESTED_APPS,
  type FirstRunV2Progress,
  type FirstRunV2Step,
} from "./firstRunV2";
import { OPEN_EVENT, onboard, type ImportSource } from "./onboardApi";
import { StepTracker } from "./Progress";
import { loadRailPrefs, saveRailPrefs, type RailMode } from "./railPrefs";
import { useModalFocus } from "./ui";

export const TITLE = `Finish setting up ${PRODUCT_NAME}`;
export const BODY =
  "A few optional steps. Skip any of them. You can run this again from Settings → Personal.";
export const SANDBOX_NOTE =
  "Shell commands stay sandboxed by default. This step never turns the sandbox off.";
export const SANDBOX_KEEP =
  "Keep the sandbox on. You can change it later in shell_sandbox.toml if you need to.";

const TRACK_STEPS = FIRST_RUN_V2_ORDER.filter((id) => id !== "done").map((id) => ({
  id,
  label: FIRST_RUN_V2_LABELS[id],
}));

/** Suggested skills to offer: cabin skills that help a first day. */
const SKILL_HINTS = ["how", "viper-mode", "tdd", "verify-this", "land-it", "qa-walkthrough"];

export function FirstRunV2({
  after,
  onAsk,
  onLayout,
}: {
  /** True once Auto Setup's first-run window is closed. */
  after: boolean;
  /** Send a first chat message (opens Chat). */
  onAsk: (text: string) => void;
  /** Apply Simple / Advanced immediately on the rail. */
  onLayout: (mode: RailMode) => void;
}) {
  const [progress, setProgress] = useState<FirstRunV2Progress>(() => loadFirstRunV2());
  const [onboardPending, setOnboardPending] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [mode, setMode] = useState<RailMode>(() => loadRailPrefs().mode);
  const [apps, setApps] = useState<CatalogRow[] | null>(null);
  const [pickedApps, setPickedApps] = useState<Set<string>>(() => new Set());
  const [skills, setSkills] = useState<SkillRow[] | null>(null);
  const [pickedSkills, setPickedSkills] = useState<Set<string>>(() => new Set());
  const [local, setLocal] = useState<LocalAiView | null>(null);
  const [sources, setSources] = useState<ImportSource[]>([]);
  const [busy, setBusy] = useState(false);
  const box = useRef<HTMLDivElement>(null);

  const shown =
    after && !onboardPending && progress.stage === "pending" && progress.step !== "done";

  const takeProgress = useCallback((next: FirstRunV2Progress) => {
    setProgress(next);
    saveFirstRunV2(next);
  }, []);

  // Wait until Get to know you is no longer pending.
  useEffect(() => {
    if (!after) return;
    let live = true;
    onboard({ act: "view" }).then(
      (reply) => {
        if (!live) return;
        setOnboardPending(reply?.pending === true);
        if (reply?.importSources) setSources(reply.importSources);
      },
      () => {
        if (live) setOnboardPending(false);
      },
    );
    return () => {
      live = false;
    };
  }, [after]);

  useEffect(() => {
    const onOpen = (e: Event) => {
      const detail = (e as CustomEvent<{ firstRunV2?: boolean; import?: boolean } | undefined>)
        .detail;
      if (detail?.firstRunV2) {
        setOnboardPending(false);
        takeProgress(reopenFirstRunV2());
      }
    };
    const onProgress = (e: Event) => {
      const detail = (e as CustomEvent<FirstRunV2Progress>).detail;
      if (detail) setProgress(detail);
    };
    window.addEventListener(OPEN_EVENT, onOpen);
    window.addEventListener(FIRST_RUN_V2_EVENT, onProgress);
    return () => {
      window.removeEventListener(OPEN_EVENT, onOpen);
      window.removeEventListener(FIRST_RUN_V2_EVENT, onProgress);
    };
  }, [takeProgress]);

  // When Get to know you closes (skip/save), re-check so this wizard can open.
  useEffect(() => {
    if (!after || !onboardPending) return;
    const timer = window.setInterval(() => {
      onboard({ act: "view" }).then(
        (reply) => {
          setOnboardPending(reply?.pending === true);
          if (reply?.importSources) setSources(reply.importSources);
        },
        () => setOnboardPending(false),
      );
    }, 800);
    return () => window.clearInterval(timer);
  }, [after, onboardPending]);

  useEffect(() => {
    if (shown) box.current?.focus();
  }, [shown, progress.step]);
  useModalFocus(box, shown);

  // Load step data when that step is current.
  useEffect(() => {
    if (!shown) return;
    const step = progress.step;
    if (step === "apps" && apps === null) {
      connectorCatalog(false).then(
        (view) => {
          const wanted = new Set(SUGGESTED_APPS.map((a) => a.id));
          setApps(view.rows.filter((r) => wanted.has(r.id)).slice(0, 8));
        },
        (err) => setError(String(err)),
      );
    }
    if (step === "skills" && skills === null) {
      getSkills().then(
        (page) => {
          const ranked = [...page.rows].sort((a, b) => {
            const ai = SKILL_HINTS.indexOf(a.name);
            const bi = SKILL_HINTS.indexOf(b.name);
            return (ai === -1 ? 99 : ai) - (bi === -1 ? 99 : bi);
          });
          setSkills(ranked.slice(0, 8));
          setPickedSkills(new Set(ranked.filter((s) => s.on).map((s) => s.name).slice(0, 8)));
        },
        (err) => setError(String(err)),
      );
    }
    if (step === "local" && local === null) {
      getLocalAi().then(setLocal, (err) => setError(String(err)));
    }
    if (step === "import" && sources.length === 0) {
      onboard({ act: "view" }).then(
        (reply) => setSources(reply?.importSources ?? []),
        () => {},
      );
    }
  }, [shown, progress.step, apps, skills, local, sources.length]);

  if (!shown) return null;

  const step = progress.step;
  const go = (to: FirstRunV2Step) => takeProgress({ stage: "pending", step: to });
  const skipAll = () => {
    takeProgress(finishFirstRunV2(true));
  };
  const finish = () => {
    takeProgress(finishFirstRunV2(false));
  };
  const back = prevStep(step);
  const forward = nextStep(step);

  let title = TITLE;
  let body;
  switch (step) {
    case "layout":
      title = "Pick a layout";
      body = (
        <>
          <p>Simple stays chat-first. Advanced puts every page on the rail.</p>
          <div className="first-run-choices" role="group" aria-label="Layout">
            {(["simple", "advanced"] as const).map((m) => (
              <button
                key={m}
                type="button"
                className="chip"
                aria-pressed={mode === m}
                onClick={() => {
                  setMode(m);
                  const prefs = { ...loadRailPrefs(), mode: m };
                  saveRailPrefs(prefs);
                  onLayout(m);
                }}
              >
                {m === "simple" ? "Simple" : "Advanced"}
              </button>
            ))}
          </div>
          <p className="note">{layoutBlurb(mode)}</p>
        </>
      );
      break;
    case "apps":
      title = "Connect apps";
      body = (
        <>
          <p>Pick up to 8. Each uses the MCPs &amp; Connectors flow. Skip any you do not need.</p>
          {apps === null && !error && <p className="note">Loading catalog…</p>}
          <ul className="first-run-list" aria-label="Suggested apps">
            {(apps ?? []).map((row) => {
              const why = SUGGESTED_APPS.find((a) => a.id === row.id)?.why ?? row.description;
              const on = pickedApps.has(row.id) || row.installed;
              return (
                <li key={row.id}>
                  <span>
                    <strong>{row.name}</strong>
                    <span className="note">{why}</span>
                  </span>
                  <button
                    type="button"
                    className="chip"
                    disabled={busy || row.installed}
                    aria-pressed={on}
                    onClick={() => void toggleApp(row)}
                  >
                    {row.installed ? "Connected" : on ? "Selected" : "Add"}
                  </button>
                </li>
              );
            })}
          </ul>
        </>
      );
      break;
    case "skills":
      title = "Pick skills";
      body = (
        <>
          <p>Turn on skills you want chat to use. You can change them later on the Skills page.</p>
          {skills === null && !error && <p className="note">Loading skills…</p>}
          <ul className="first-run-list" aria-label="Skills">
            {(skills ?? []).map((row) => {
              const on = pickedSkills.has(row.name);
              return (
                <li key={row.name}>
                  <span>
                    <strong>{row.name}</strong>
                    <span className="note">{row.description || row.trigger}</span>
                  </span>
                  <button
                    type="button"
                    className="chip"
                    disabled={busy}
                    aria-pressed={on}
                    onClick={() => void toggleSkill(row, !on)}
                  >
                    {on ? "On" : "Off"}
                  </button>
                </li>
              );
            })}
          </ul>
        </>
      );
      break;
    case "local":
      title = "Local model";
      body = <LocalOffer view={local} />;
      break;
    case "import":
      title = "Import notes";
      body = (
        <>
          <p>
            Pull notes from OpenClaw, Hermes, Claude Code, Cursor or Codex when they are on this
            computer. Nothing is saved until you review in Get to know you.
          </p>
          {sources.length === 0 ? (
            <p className="note">No importable workspace found on this computer.</p>
          ) : (
            <ul className="first-run-list" aria-label="Import sources">
              {sources.map((s) => (
                <li key={s.id}>
                  <span>
                    <strong>{s.label}</strong>
                    <span className="note">Found on this computer</span>
                  </span>
                  <button
                    type="button"
                    className="chip"
                    disabled={busy}
                    onClick={() => void importSource(s)}
                  >
                    Add notes
                  </button>
                </li>
              ))}
            </ul>
          )}
        </>
      );
      break;
    case "sandbox":
      title = "Sandbox stays on";
      body = (
        <>
          <p>{SANDBOX_NOTE}</p>
          <p className="note">{SANDBOX_KEEP}</p>
        </>
      );
      break;
    case "task":
      title = "Your first task";
      body = (
        <>
          <p>Pick one. Chat opens and sends it so you get an answer without opening Settings.</p>
          <div className="first-run-choices" role="group" aria-label="First task">
            {FIRST_TASKS.map((t) => (
              <button
                key={t.id}
                type="button"
                onClick={() => {
                  finish();
                  onAsk(t.prompt);
                }}
              >
                {t.label}
              </button>
            ))}
          </div>
        </>
      );
      break;
    case "tour":
      title = "Quick tour";
      body = (
        <>
          <p>
            The left rail opens Chat, Code, Pulse and more. Press Ctrl+K for the palette. Connectors
            and Skills sit under Make in Advanced, or under More in Simple.
          </p>
          <p className="note">Next, pick a first task — or skip and explore on your own.</p>
        </>
      );
      break;
    case "done":
      title = "You're ready";
      body = <p>Open Chat whenever you like. Re-run setup from Settings → Personal.</p>;
      break;
  }

  async function toggleApp(row: CatalogRow) {
    if (row.installed || pickedApps.has(row.id)) {
      setPickedApps((set) => {
        const next = new Set(set);
        next.delete(row.id);
        return next;
      });
      return;
    }
    if (pickedApps.size >= 8) {
      setError("You can connect up to 8 suggested apps here.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const changed = await addCatalogConnector(row.id);
      setPickedApps((set) => new Set(set).add(row.id));
      setApps((list) =>
        list?.map((r) =>
          r.id === row.id ? { ...r, installed: changed.rows.some((c) => c.name === row.id) } : r,
        ) ?? list,
      );
      if (row.auth === "oauth" || row.auth === "google") {
        const name = changed.rows.find((c) => c.name === row.id)?.name ?? row.id;
        await signInConnector(name).catch(() => {});
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function toggleSkill(row: SkillRow, on: boolean) {
    setBusy(true);
    setError(null);
    try {
      await setSkillOn(row.name, on);
      setPickedSkills((set) => {
        const next = new Set(set);
        if (on) next.add(row.name);
        else next.delete(row.name);
        return next;
      });
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function importSource(source: ImportSource) {
    setBusy(true);
    setError(null);
    try {
      const draft = await onboard({
        act: "draft",
        answers: {
          name: "",
          callMe: "",
          about: "",
          tone: "",
          soulNotes: "",
          goals: "",
          interests: "",
          routines: "",
          apps: "",
          prefs: "",
          remember: "",
        },
      });
      if (!draft?.drafts) {
        setError("Could not draft notes for import.");
        return;
      }
      const next = await onboard({
        act: "openclaw",
        drafts: draft.drafts,
        source: source.id,
      });
      if (next?.note) setError(next.note);
      else setError(`Added notes from ${source.label}. Review them in Settings → Personal.`);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="first-run-backdrop">
      <div
        ref={box}
        tabIndex={-1}
        className="card first-run onboard first-run-v2"
        role="dialog"
        aria-modal="true"
        aria-labelledby="first-run-v2-title"
        onKeyDown={(e) => {
          if (e.key === "Escape") skipAll();
        }}
      >
        <h2 id="first-run-v2-title">{title}</h2>
        <StepTracker steps={TRACK_STEPS} current={step} label="First-run steps" />
        {step === "layout" && <p className="note">{BODY}</p>}
        {body}
        {error && <p className="note warn">{error}</p>}
        <div className="first-run-foot">
          {back && (
            <button type="button" className="chip" onClick={() => go(back)}>
              Back
            </button>
          )}
          <button type="button" className="chip" onClick={skipAll}>
            Skip setup
          </button>
          {step !== "task" && step !== "done" && (
            <button type="button" onClick={() => go(forward)}>
              Next
            </button>
          )}
          {step === "task" && (
            <button type="button" className="chip" onClick={finish}>
              Skip task
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

function LocalOffer({ view }: { view: LocalAiView | null }) {
  if (view === null) return <p className="note">Checking this PC…</p>;
  const can =
    view.picker.models.some((m) => m.bucket === "recommended" || m.bucket === "maySlow") ||
    view.engine.state === "ready" ||
    view.engine.state === "missing" ||
    view.status.state !== "Not set up";
  if (!can) {
    return (
      <p>
        This PC does not look ready for an on-device model yet. You can set one up later in Settings
        → Local AI.
      </p>
    );
  }
  return (
    <>
      <p>
        This PC can run a local model. Open Settings → Local AI when you want one — or skip and keep
        using Grok.
      </p>
      <p className="note">
        Status: {view.status.state}
        {view.status.model ? ` · ${view.status.model}` : ""}. Engine: {view.engine.state}.
      </p>
    </>
  );
}

import { useCallback, useEffect, useEffectEvent, useRef, useState } from "react";
import {
  cancelLocalDrafterDownload,
  cancelLocalEngineDownload,
  checkModelFile,
  getLocalAi,
  onLocalDrafterEvent,
  onLocalEngineEvent,
  resetLocalAi,
  runLocalSpeedTest,
  saveLocalAi,
  setLocalDynamic,
  setPromptTidy,
  setLocalKeepLoaded,
  setLocalUse,
  startLocalDrafterDownload,
  startLocalEngineDownload,
  unloadLocalModel,
  type LocalAiConfig,
  type LocalAiView,
  type LocalDraftOffer,
  type LocalDraftProgress,
  type LocalDrafterState,
  type LocalEngineEventState,
  type LocalEngineInfo,
  type LocalFileRow,
  type LocalManual,
} from "./api";
import { ModelLibrary, type LibraryFocus } from "./ModelLibrary";
import { Toggle } from "./Toggle";
import { VoiceRouteSettings } from "./VoiceRouteSettings";
import { WakeWordSettings } from "./WakeWordSettings";
import { PRODUCT_NAME } from "./brand";

/** Shown once per file before a model file from outside the catalog is added. */
export const MODEL_NOTICE =
  `This model isn't from ${PRODUCT_NAME}'s catalog. Third-party models can have their safety training removed. Check its license before you use it. You are responsible for what you do with it. ${PRODUCT_NAME} never recommends or downloads models from outside its catalog.`;

/** Shown once per file before a catalog build with its safety training removed is picked. */
export const ABLITERATED_NOTICE =
  "This build's safety training was removed, so it answers what a stock model would refuse. Check its license before you use it. You are responsible for what you do with it.";

/** A line under the panel: what just happened, or what went wrong. */
interface Note {
  text: string;
  error: boolean;
}

/** The engine button for `state`: Cancel while downloading, nothing once ready,
 * else the Get or Resume label the backend gave before the download. */
function engineButton(state: LocalEngineEventState, idle: string): string {
  if (state === "downloading") return "Cancel";
  if (state === "ready") return "";
  return idle;
}

/** Blank is Auto. */
function numberOrNull(text: string): number | null {
  const n = Number.parseInt(text, 10);
  return Number.isFinite(n) && n > 0 ? n : null;
}

function lines(text: string): string[] {
  return text
    .split("\n")
    .map((l) => l.trim())
    .filter(Boolean);
}

function fileName(path: string): string {
  return path.split(/[\\/]/).pop() || path;
}

/** A model file found on this PC or checked from a path, offered to add. */
function FoundFile({ row, onAdd }: { row: LocalFileRow; onAdd: () => void }) {
  return (
    <li className="local-model file" aria-label={row.name}>
      <span className="local-model-name">{row.name}</span>
      {row.abliterated && <span className="badge warn">Safety training removed</span>}
      {row.thirdParty && !row.abliterated && <span className="badge">Third-party</span>}
      <span className="local-model-meta">
        {row.source} · {row.size} · {row.path}
      </span>
      <button type="button" className="quiet add-file" onClick={onAdd}>
        Add
      </button>
    </li>
  );
}

const READY_ENGINE: LocalEngineInfo = {
  state: "ready",
  version: "",
  sizeLabel: "",
  hint: "",
  button: "",
  progress: null,
  error: null,
};

export function LocalAi({ focus }: { focus?: LibraryFocus } = {}) {
  const [view, setView] = useState<LocalAiView | null>(null);
  const [draft, setDraft] = useState<LocalAiConfig | null>(null);
  const [engine, setEngine] = useState<LocalEngineInfo>(READY_ENGINE);
  const [draftOffer, setDraftOffer] = useState<LocalDraftOffer | null>(null);
  const [draftProgress, setDraftProgress] = useState<LocalDraftProgress | null>(null);
  const [draftState, setDraftState] = useState<LocalDrafterState | null>(null);
  /** The drafter last seen downloading, so a paused one can resume. */
  const [draftId, setDraftId] = useState<string | null>(null);
  const [filePath, setFilePath] = useState("");
  const [extraFiles, setExtraFiles] = useState<LocalFileRow[]>([]);
  const [notice, setNotice] = useState<LocalFileRow | null>(null);
  const [note, setNote] = useState<Note | null>(null);
  const [testing, setTesting] = useState(false);
  const [allow, setAllow] = useState("");
  const [deny, setDeny] = useState("");
  /** The Get or Resume label to show again once a download stops. */
  const idleButton = useRef("");

  const show = useCallback((next: LocalAiView) => {
    setView(next);
    setDraft(next.config);
    setEngine(next.engine);
    if (next.engine.state !== "downloading") idleButton.current = next.engine.button;
    setDraftOffer(next.draftOffer);
    setDraftProgress(next.draftProgress);
    setDraftId(next.draftProgress?.id ?? null);
    setDraftState(null);
    setAllow(next.config.manual.allowApps.join("\n"));
    setDeny(next.config.manual.denyApps.join("\n"));
  }, []);

  const tell = (text: string) => setNote({ text, error: false });
  const fail = (err: unknown) => setNote({ text: String(err), error: true });

  // Runs each time this section shows. With unsaved edits the form stays as it
  // is and only the status refreshes.
  const refresh = useEffectEvent((next: LocalAiView) => {
    const base = view?.config;
    const edited =
      base !== undefined &&
      (draft !== base ||
        allow !== base.manual.allowApps.join("\n") ||
        deny !== base.manual.denyApps.join("\n"));
    if (!edited) return show(next);
    setView(next);
    setEngine(next.engine);
    setDraftOffer(next.draftOffer);
    setDraftProgress(next.draftProgress);
  });
  useEffect(() => {
    getLocalAi().then(
      (next) => refresh(next),
      (err: unknown) => setNote({ text: String(err), error: true }),
    );
  }, []);

  useEffect(() => {
    let stop: (() => void) | undefined;
    let dropped = false;
    void onLocalEngineEvent((event) => {
      setEngine((now) => ({
        ...now,
        state: event.state,
        progress: event.progress,
        error: event.error,
        button: engineButton(event.state, idleButton.current),
      }));
      if (event.message) setNote({ text: event.message, error: event.error !== null });
      if (event.state === "ready" || event.state === "missing") {
        void getLocalAi().then(show).catch(() => undefined);
      }
    }).then((unlisten) => {
      if (dropped) unlisten();
      else stop = unlisten;
    });
    return () => {
      dropped = true;
      stop?.();
    };
  }, [show]);

  useEffect(() => {
    let stop: (() => void) | undefined;
    let dropped = false;
    void onLocalDrafterEvent((event) => {
      setDraftProgress(event.progress);
      if (event.progress) setDraftId(event.progress.id);
      setDraftState(event.state);
      if (event.message) setNote({ text: event.message, error: event.error !== null });
      if (event.state !== "downloading") {
        void getLocalAi().then(show).catch(() => undefined);
      }
    }).then((unlisten) => {
      if (dropped) unlisten();
      else stop = unlisten;
    });
    return () => {
      dropped = true;
      stop?.();
    };
  }, [show]);

  const noteClass = `note local-note${note?.error ? " error" : ""}`;

  if (!view || !draft) {
    return (
      <section className="local-ai" aria-label="Local AI">
        <h2>Local AI</h2>
        <p className={noteClass} role="status">
          {note?.text ?? "Reading this PC's hardware…"}
        </p>
      </section>
    );
  }

  const config = draft;
  const manual = config.manual;
  const set = (over: Partial<LocalAiConfig>) => setDraft({ ...config, ...over });
  const setManual = (over: Partial<LocalManual>) =>
    setDraft({ ...config, manual: { ...manual, ...over } });
  const addPath = (path: string, seen: string[] = config.noticeSeen) =>
    set({ ownFiles: [...config.ownFiles.filter((f) => f !== path), path], noticeSeen: seen });
  const removePath = (path: string) => set({ ownFiles: config.ownFiles.filter((f) => f !== path) });

  function addFile(row: LocalFileRow) {
    if (row.needsNotice && !config.noticeSeen.includes(row.path)) {
      setNotice(row);
      return;
    }
    addPath(row.path);
  }

  function acceptNotice() {
    if (!notice) return;
    addPath(notice.path, [...config.noticeSeen, notice.path]);
    setNotice(null);
  }

  async function checkFile() {
    try {
      const row = await checkModelFile(filePath.trim());
      setExtraFiles((rows) => [...rows.filter((r) => r.path !== row.path), row]);
      setNote(null);
      setFilePath("");
      addFile(row);
    } catch (err) {
      fail(err);
    }
  }

  async function save() {
    try {
      show(
        await saveLocalAi({
          ...config,
          manual: { ...manual, allowApps: lines(allow), denyApps: lines(deny) },
        }),
      );
      tell("Local AI saved.");
    } catch (err) {
      fail(err);
    }
  }

  async function reset() {
    try {
      show(await resetLocalAi());
      tell("Advanced settings are back to their defaults.");
    } catch (err) {
      fail(err);
    }
  }

  async function dynamic(on: boolean) {
    try {
      setView(await setLocalDynamic(on));
      tell(on ? "Dynamic is on: each task gets the model best at it." : "Dynamic is off: one model answers every task.");
    } catch (err) {
      fail(err);
    }
  }

  async function switchLocal(on: boolean) {
    try {
      setView(await setLocalUse(on));
      tell(on ? "Local AI is on for this computer." : "Local AI is off: nothing runs on the on-device model.");
    } catch (err) {
      fail(err);
    }
  }

  async function tidy(on: boolean) {
    try {
      setView(await setPromptTidy(on));
      tell(on ? "Typo tidy is on: the on-device model fixes typos while it's loaded." : "Typo tidy is off: your messages go as you type them.");
    } catch (err) {
      fail(err);
    }
  }

  async function keepLoaded(on: boolean) {
    try {
      setView(await setLocalKeepLoaded(on));
    } catch (err) {
      fail(err);
    }
  }

  async function unloadNow() {
    try {
      setView(await unloadLocalModel());
      tell(view?.localUse.on === false ? "Unloaded the model." : "Unloaded the model. It loads again with the next reply.");
    } catch (err) {
      fail(err);
    }
  }

  async function speedTest() {
    setTesting(true);
    try {
      tell(await runLocalSpeedTest());
    } catch (err) {
      fail(err);
    } finally {
      setTesting(false);
    }
  }

  async function getEngine() {
    try {
      if (engine.state === "downloading") {
        await cancelLocalEngineDownload();
        tell("Cancelled the engine download.");
        return;
      }
      const started = await startLocalEngineDownload();
      if (started.state !== "downloading") idleButton.current = started.button;
      setEngine(started);
    } catch (err) {
      fail(err);
    }
  }

  async function installDraft(targetId: string) {
    try {
      if (draftProgress) {
        await cancelLocalDrafterDownload();
        tell("Cancelled the draft model download.");
        return;
      }
      await startLocalDrafterDownload(targetId);
    } catch (err) {
      fail(err);
    }
  }

  const s = view.status;
  const status = [s.tier, s.model, `${s.state} (${s.reason})`, s.tokS, s.memory]
    .filter(Boolean)
    .join(" · ");
  const found = [
    ...view.picker.onThisPc,
    ...extraFiles.filter((f) => !view.picker.onThisPc.some((o) => o.path === f.path)),
  ].filter((f) => !config.ownFiles.includes(f.path));
  const paused = draftState === "paused";
  const draftTarget =
    view.draftOffers.find((o) => o.id === draftId)?.targetId ??
    draftOffer?.targetId ??
    view.picker.selected;
  const draftBlock = (
    <div className="local-draft-offer" aria-label="Draft model offer">
      <h3>Draft model</h3>
      {draftOffer && <p className="note">{draftOffer.line}</p>}
      {draftProgress && <p className="note local-draft-progress">{draftProgress.line}</p>}
      <button
        type="button"
        className="quiet install-draft"
        onClick={() => void installDraft(draftTarget)}
      >
        {draftProgress
          ? "Cancel draft download"
          : paused
            ? "Resume…"
            : (draftOffer?.button ?? "Install and run draft model")}
      </button>
    </div>
  );

  return (
    <section className="local-ai" aria-label="Local AI">
      <h2>Local AI</h2>
      <p className="local-status">{status}</p>
      <div className="local-running" aria-label="Running now">
        {view.running.lines.map((line) => (
          <p key={line} className="note">
            {line}
          </p>
        ))}
        <button
          type="button"
          className="quiet"
          onClick={() => void keepLoaded(!view.running.keptLoaded)}
          disabled={!view.localUse.on}
        >
          {view.running.keptLoaded ? "Let it unload" : "Keep loaded"}
        </button>
        <p className="note">{view.running.dynamicHint}</p>
        <button type="button" className="quiet" onClick={() => void dynamic(!view.running.dynamic)}>
          {view.running.dynamic ? "Turn off Dynamic" : "Turn on Dynamic"}
        </button>
        {view.running.loaded && (
          <button type="button" className="quiet" onClick={() => void unloadNow()}>
            Unload now
          </button>
        )}
      </div>
      <div className="local-use" role="group" aria-label="Use Local AI">
        <Toggle checked={view.localUse.on} onChange={(on) => void switchLocal(on)}>
          Use Local AI on this computer
        </Toggle>
        <p className="note">{view.localUse.hint}</p>
      </div>
      <div className="local-tidy" role="group" aria-label="Typo tidy">
        <Toggle checked={view.tidy.on} disabled={!view.tidy.available && !view.tidy.on} onChange={(on) => void tidy(on)}>
          Tidy typos on this computer (can change wording)
        </Toggle>
        <p className="note">{view.tidy.hint}</p>
      </div>
      {view.limits.length > 0 && (
        <div className="local-limits">
          <p className="note">Detection is limited on this PC:</p>
          <ul>
            {view.limits.map((l) => (
              <li key={l} className="note">
                {l}
              </li>
            ))}
          </ul>
        </div>
      )}
      <div className="local-engine" aria-label="Local engine">
        <h3>Local engine</h3>
        <p className="note">{engine.hint || "Checking the local engine…"}</p>
        {engine.progress && <p className="note local-engine-progress">{engine.progress.line}</p>}
        {engine.error && (
          <p className="note error local-engine-error" role="status">
            {engine.error}
          </p>
        )}
        {engine.button && (
          <button type="button" className="quiet get-engine" onClick={() => void getEngine()}>
            {engine.button}
          </button>
        )}
      </div>
      <Toggle checked={config.pauseForGames} onChange={(on) => set({ pauseForGames: on })}>
        Pause while gaming or a fullscreen app is in front
      </Toggle>
      <Toggle checked={config.useIdleTime} onChange={(on) => set({ useIdleTime: on })}>
        Use idle time to run faster
      </Toggle>
      <Toggle checked={config.throttleOnBattery} onChange={(on) => set({ throttleOnBattery: on })}>
        Slow down on battery (it pauses below 20%)
      </Toggle>
      {/* A running download keeps its progress and Cancel in view. */}
      {(draftProgress || paused) && draftBlock}
      <details className="local-advanced">
        <summary>Advanced</summary>
        <div className="local-manual">
          <p className="note">
            Auto picks the model for this PC. These settings only change how it runs.
          </p>
          <h3>Your model files</h3>
          <p className="note">
            Auto loads the first added file that is on disk when no downloaded model is on this PC.
          </p>
          {config.ownFiles.length > 0 && (
            <ul aria-label="Added model files">
              {config.ownFiles.map((path) => (
                <li key={path} className="local-model file" aria-label={fileName(path)}>
                  <span className="local-model-name">{fileName(path)}</span>
                  <span className="local-model-meta">{path}</span>
                  <button type="button" className="quiet remove-file" onClick={() => removePath(path)}>
                    Remove
                  </button>
                </li>
              ))}
            </ul>
          )}
          {found.length > 0 && (
            <div className="local-bucket files">
              <h4>On this PC</h4>
              <ul aria-label="On this PC">
                {found.map((row) => (
                  <FoundFile key={row.path} row={row} onAdd={() => addFile(row)} />
                ))}
              </ul>
            </div>
          )}
          <div className="local-file">
            <input
              aria-label="Model file path"
              placeholder="/path/to/model.gguf"
              value={filePath}
              onChange={(e) => setFilePath(e.target.value)}
            />
            <button
              type="button"
              className="quiet use-file"
              onClick={checkFile}
              disabled={!filePath.trim()}
            >
              Add a model file
            </button>
          </div>
          {notice && (
            <div className="notice" role="alertdialog" aria-label="About this model">
              <p>
                <strong>{notice.name}</strong>
              </p>
              <p>{notice.thirdParty ? MODEL_NOTICE : ABLITERATED_NOTICE}</p>
              <div className="card-buttons">
                <button type="button" className="accept" onClick={acceptNotice}>
                  I understand
                </button>
                <button type="button" className="quiet" onClick={() => setNotice(null)}>
                  Cancel
                </button>
              </div>
            </div>
          )}
          <h3>Engine</h3>
          <div className="local-grid">
            <label>
              GPU layers
              <input
                inputMode="numeric"
                placeholder="Auto"
                value={manual.ngl ?? ""}
                onChange={(e) => setManual({ ngl: numberOrNull(e.target.value) })}
              />
            </label>
            <label>
              Context
              <input
                inputMode="numeric"
                placeholder="Auto"
                value={manual.ctx ?? ""}
                onChange={(e) => setManual({ ctx: numberOrNull(e.target.value) })}
              />
            </label>
            <label>
              Threads
              <input
                inputMode="numeric"
                placeholder={`Auto (up to ${view.maxThreads})`}
                value={manual.threads ?? ""}
                onChange={(e) => setManual({ threads: numberOrNull(e.target.value) })}
              />
            </label>
            <label>
              KV cache
              <select value={manual.kv} onChange={(e) => setManual({ kv: e.target.value })}>
                <option value="auto">Auto</option>
                <option value="q8">Q8</option>
                <option value="q4">Q4</option>
              </select>
            </label>
            <label>
              Unload after idle (minutes)
              <input
                inputMode="numeric"
                value={manual.idleMinutes}
                disabled={manual.neverUnload}
                onChange={(e) => setManual({ idleMinutes: numberOrNull(e.target.value) ?? 1 })}
              />
            </label>
          </div>
          <Toggle checked={manual.neverUnload} onChange={(on) => setManual({ neverUnload: on })}>
            Never unload the model
          </Toggle>
          <h3>Apps</h3>
          <label>
            Never pause for these apps (one per line)
            <textarea rows={2} value={allow} onChange={(e) => setAllow(e.target.value)} />
          </label>
          <label>
            Always pause for these apps (one per line)
            <textarea rows={2} value={deny} onChange={(e) => setDeny(e.target.value)} />
          </label>
          <button type="button" className="quiet" onClick={speedTest} disabled={testing || !view.localUse.on}>
            {testing ? "Testing…" : "Re-run speed test"}
          </button>
          {!draftProgress && !paused && draftOffer && draftBlock}
        </div>
      </details>
      <div className="card-buttons">
        <button type="button" className="save-local" onClick={save}>
          Save local AI
        </button>
        <button type="button" className="quiet reset-local" onClick={reset}>
          Reset Advanced
        </button>
      </div>
      {note && (
        <p className={noteClass} role="status">
          {note.text}
        </p>
      )}
      <VoiceRouteSettings />
      <WakeWordSettings />
      <ModelLibrary focus={focus} />
    </section>
  );
}

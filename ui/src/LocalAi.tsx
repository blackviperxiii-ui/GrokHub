import { useEffect, useState } from "react";
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
  startLocalDrafterDownload,
  startLocalEngineDownload,
  type LocalAiConfig,
  type LocalAiView,
  type LocalDraftOffer,
  type LocalDraftProgress,
  type LocalEngineInfo,
  type LocalFileRow,
  type LocalManual,
  type LocalModelRow,
} from "./api";
import { Toggle } from "./Toggle";
import { PRODUCT_NAME } from "./brand";

/** Shown once per file before a model from outside the catalog is picked. */
export const MODEL_NOTICE =
  `This model isn't from ${PRODUCT_NAME}'s catalog. Third-party models can have their safety training removed. Check its license before you use it. You are responsible for what you do with it. ${PRODUCT_NAME} never recommends or downloads models from outside its catalog.`;

const BACKENDS: [string, string][] = [
  ["auto", "Auto"],
  ["cuda", "CUDA (NVIDIA)"],
  ["rocm", "ROCm (AMD)"],
  ["vulkan", "Vulkan"],
  ["metal", "Metal (Apple)"],
  ["sycl", "SYCL (Intel)"],
  ["cpu", "CPU only"],
];

const CAP_MAX = 90;

function ctxLabel(ctx: number): string {
  return ctx >= 1024 ? `${Math.round(ctx / 1024)}K ctx` : `${ctx} ctx`;
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

function ModelOption({
  row,
  selected,
  onPick,
}: {
  row: LocalModelRow;
  selected: boolean;
  onPick: () => void;
}) {
  const meta = [row.size, row.quant, ctxLabel(row.ctx), row.tokS, row.license].join(" · ");
  return (
    <li className={`local-model ${row.bucket}`} aria-label={row.name}>
      <label className="check">
        <input type="radio" name="local-model" checked={selected} onChange={onPick} />
        <span className="local-model-name">{row.name}</span>
        {row.preselected && <span className="badge">Default for this PC</span>}
        {row.optional && <span className="badge">Optional</span>}
        {row.badges.map((b) => (
          <span key={b} className="badge">
            {b}
          </span>
        ))}
      </label>
      <span className="local-model-meta">{meta}</span>
      <span className="local-model-reason">{row.reason}</span>
    </li>
  );
}

function FileOption({
  row,
  selected,
  onPick,
}: {
  row: LocalFileRow;
  selected: boolean;
  onPick: () => void;
}) {
  return (
    <li className="local-model file" aria-label={row.name}>
      <label className="check">
        <input type="radio" name="local-model" checked={selected} onChange={onPick} />
        <span className="local-model-name">{row.name}</span>
        {row.abliterated && <span className="badge warn">Safety training removed</span>}
        {row.thirdParty && !row.abliterated && <span className="badge">Third-party</span>}
      </label>
      <span className="local-model-meta">
        {row.source} · {row.size} · {row.path}
      </span>
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

export function LocalAi() {
  const [view, setView] = useState<LocalAiView | null>(null);
  const [draft, setDraft] = useState<LocalAiConfig | null>(null);
  const [engine, setEngine] = useState<LocalEngineInfo>(READY_ENGINE);
  const [draftOffer, setDraftOffer] = useState<LocalDraftOffer | null>(null);
  const [draftProgress, setDraftProgress] = useState<LocalDraftProgress | null>(null);
  const [showAll, setShowAll] = useState(false);
  const [filePath, setFilePath] = useState("");
  const [extraFiles, setExtraFiles] = useState<LocalFileRow[]>([]);
  const [notice, setNotice] = useState<LocalFileRow | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);
  const [allow, setAllow] = useState("");
  const [deny, setDeny] = useState("");

  function show(next: LocalAiView) {
    setView(next);
    setDraft(next.config);
    setEngine(next.engine);
    setDraftOffer(next.draftOffer);
    setDraftProgress(next.draftProgress);
    setAllow(next.config.manual.allowApps.join("\n"));
    setDeny(next.config.manual.denyApps.join("\n"));
  }

  useEffect(() => {
    getLocalAi().then(show, (err) => setNote(String(err)));
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
        button: event.state === "downloading" ? "Cancel" : now.button,
      }));
      if (event.message) setNote(event.message);
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
  }, []);

  useEffect(() => {
    let stop: (() => void) | undefined;
    let dropped = false;
    void onLocalDrafterEvent((event) => {
      setDraftProgress(event.progress);
      if (event.message) setNote(event.message);
      if (event.state === "done" || event.state === "error" || event.state === "cancelled") {
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
  }, []);

  if (!view || !draft) {
    return (
      <section className="local-ai" aria-label="Local AI">
        <h2>Local AI</h2>
        <p className="note">{note ?? "Reading this PC's hardware…"}</p>
      </section>
    );
  }

  const config = draft;
  const manual = config.manual;
  const selected = config.localModel.selected || view.picker.selected;
  const set = (over: Partial<LocalAiConfig>) => setDraft({ ...config, ...over });
  const setManual = (over: Partial<LocalManual>) =>
    setDraft({ ...config, manual: { ...manual, ...over } });
  const pick = (value: string) => set({ localModel: { selected: value } });

  function pickFile(row: LocalFileRow) {
    if (row.needsNotice && !config.noticeSeen.includes(row.path)) {
      setNotice(row);
      return;
    }
    pick(row.path);
  }

  function acceptNotice() {
    if (!notice) return;
    setDraft({
      ...config,
      localModel: { selected: notice.path },
      noticeSeen: [...config.noticeSeen, notice.path],
    });
    setNotice(null);
  }

  async function useFile() {
    try {
      const row = await checkModelFile(filePath.trim());
      setExtraFiles((rows) => [...rows.filter((r) => r.path !== row.path), row]);
      setNote(null);
      pickFile(row);
    } catch (err) {
      setNote(String(err));
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
      setNote("Local AI saved.");
    } catch (err) {
      setNote(String(err));
    }
  }

  async function reset() {
    try {
      show(await resetLocalAi());
      setExtraFiles([]);
      setNote("Back to Auto.");
    } catch (err) {
      setNote(String(err));
    }
  }

  async function speedTest() {
    setTesting(true);
    try {
      setNote(await runLocalSpeedTest());
    } catch (err) {
      setNote(String(err));
    } finally {
      setTesting(false);
    }
  }

  async function getEngine() {
    try {
      if (engine.state === "downloading") {
        await cancelLocalEngineDownload();
        setNote("Cancelled the engine download.");
        return;
      }
      setEngine(await startLocalEngineDownload());
    } catch (err) {
      setNote(String(err));
    }
  }

  async function installDraft(targetId: string) {
    try {
      if (draftProgress) {
        await cancelLocalDrafterDownload();
        setNote("Cancelled the draft model download.");
        return;
      }
      await startLocalDrafterDownload(targetId);
    } catch (err) {
      setNote(String(err));
    }
  }

  const s = view.status;
  const status = [s.tier, s.model, `${s.state} (${s.reason})`, s.tokS, s.memory]
    .filter(Boolean)
    .join(" · ");
  const buckets: [LocalModelRow["bucket"], string][] = [
    ["recommended", "Recommended"],
    ["maySlow", "May be slow"],
  ];
  if (showAll) buckets.push(["hidden", "Won't run well here"]);
  const files = [
    ...view.picker.onThisPc,
    ...extraFiles.filter((f) => !view.picker.onThisPc.some((o) => o.path === f.path)),
  ];
  const selectedOffer =
    view.draftOffers.find((o) => o.targetId === selected) ??
    (draftOffer?.targetId === selected ? draftOffer : null);

  return (
    <section className="local-ai" aria-label="Local AI">
      <h2>Local AI</h2>
      <p className="local-status">{status}</p>
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
        {engine.error && <p className="note local-engine-error">{engine.error}</p>}
        {engine.button && (
          <button type="button" className="quiet get-engine" onClick={() => void getEngine()}>
            {engine.button}
          </button>
        )}
      </div>
      <div className="local-mode" role="group" aria-label="Local AI mode">
        <button
          type="button"
          className="quiet mode-auto"
          aria-pressed={config.mode === "auto"}
          onClick={() => set({ mode: "auto" })}
        >
          Auto
        </button>
        <button
          type="button"
          className="quiet mode-manual"
          aria-pressed={config.mode === "manual"}
          onClick={() => set({ mode: "manual" })}
        >
          Manual
        </button>
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
      {config.mode === "manual" && (
        <div className="local-manual">
          <h3>Model</h3>
          {buckets.map(([bucket, title]) => {
            const rows = view.picker.models.filter((m) => m.bucket === bucket);
            if (rows.length === 0) return null;
            return (
              <div key={bucket} className={`local-bucket ${bucket}`}>
                <h4>{title}</h4>
                <ul aria-label={title}>
                  {rows.map((row) => (
                    <ModelOption
                      key={row.id}
                      row={row}
                      selected={selected === row.id}
                      onPick={() => pick(row.id)}
                    />
                  ))}
                </ul>
              </div>
            );
          })}
          <button type="button" className="link" onClick={() => setShowAll(!showAll)}>
            {showAll ? "Hide what won't run well" : "Show all"}
          </button>
          {files.length > 0 && (
            <div className="local-bucket files">
              <h4>On this PC</h4>
              <ul aria-label="On this PC">
                {files.map((row) => (
                  <FileOption
                    key={row.path}
                    row={row}
                    selected={selected === row.path}
                    onPick={() => pickFile(row)}
                  />
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
              onClick={useFile}
              disabled={!filePath.trim()}
            >
              Use a model file
            </button>
          </div>
          {notice && (
            <div className="notice" role="alertdialog" aria-label="About this model">
              <p>
                <strong>{notice.name}</strong>
              </p>
              <p>{MODEL_NOTICE}</p>
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
          {(selectedOffer || draftProgress) && (
            <div className="local-draft-offer" aria-label="Draft model offer">
              <h3>Draft model</h3>
              {selectedOffer && <p className="note">{selectedOffer.line}</p>}
              {draftProgress && <p className="note">{draftProgress.line}</p>}
              <button
                type="button"
                className="quiet install-draft"
                onClick={() =>
                  void installDraft(selectedOffer?.targetId ?? draftOffer?.targetId ?? selected)
                }
              >
                {draftProgress
                  ? "Cancel draft download"
                  : (selectedOffer?.button ?? "Install and run draft model")}
              </button>
            </div>
          )}
          <h3>Engine</h3>
          <div className="local-grid">
            <label>
              Device
              <select value={manual.backend} onChange={(e) => setManual({ backend: e.target.value })}>
                {BACKENDS.map(([value, label]) => (
                  <option key={value} value={value}>
                    {label}
                  </option>
                ))}
              </select>
            </label>
            <label>
              Draft model
              <select value={manual.draft} onChange={(e) => setManual({ draft: e.target.value })}>
                <option value="auto">Auto (after the speed test)</option>
                <option value="on">On</option>
                <option value="off">Off</option>
              </select>
            </label>
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
          <h3>Memory caps (up to {CAP_MAX}%)</h3>
          <div className="local-grid">
            {(
              [
                ["rampUp", "Idle"],
                ["normal", "Normal"],
                ["throttle", "Busy"],
              ] as const
            ).map(([key, label]) => (
              <label key={key}>
                {label}
                <input
                  inputMode="numeric"
                  value={manual.caps[key]}
                  onChange={(e) =>
                    setManual({
                      caps: {
                        ...manual.caps,
                        [key]: Math.min(CAP_MAX, numberOrNull(e.target.value) ?? 10),
                      },
                    })
                  }
                />
              </label>
            ))}
          </div>
          <h3>Apps</h3>
          <label>
            Never pause for these apps (one per line)
            <textarea rows={2} value={allow} onChange={(e) => setAllow(e.target.value)} />
          </label>
          <label>
            Always pause for these apps (one per line)
            <textarea rows={2} value={deny} onChange={(e) => setDeny(e.target.value)} />
          </label>
          <button type="button" className="quiet" onClick={speedTest} disabled={testing}>
            {testing ? "Testing…" : "Re-run speed test"}
          </button>
        </div>
      )}
      <div className="card-buttons">
        <button type="button" className="save-local" onClick={save}>
          Save local AI
        </button>
        <button type="button" className="quiet reset-local" onClick={reset}>
          Reset to Auto
        </button>
      </div>
      {note && <p className="note local-note">{note}</p>}
    </section>
  );
}

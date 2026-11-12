// Mods 2 of 2, the same as the cabin's Mods page: the open mod's drawer
// with its files and what each offers, the sheet that lists every file an
// install or uninstall writes and waits for a pointer click, the line that
// says where the one job stands, and the Installed tab.
import {
  useEffect,
  useEffectEvent,
  useRef,
  useState,
  type MouseEvent,
} from "react";
import { openUrl, type ModGame, type ModRow } from "./api";
import {
  modsCancel,
  modsConfirm,
  modsFiles,
  modsInstalled,
  modsPickupDownload,
  modsPlanInstall,
  modsPlanUninstall,
  modsWaitForBrowser,
  onModsJob,
  type ModDrawer,
  type ModFileRow,
  type ModInstalled,
  type ModSheet,
} from "./modsInstallApi";
import { PRODUCT_NAME } from "./brand";
import { useModalFocus } from "./ui";

export const NO_INSTALLED = `No mods found for this game yet. Mods ${PRODUCT_NAME} installs will list here too.`;
export const ON_SHEET = "Waiting for you on the sheet.";

/** Where the one install at a time stands. */
export type Step =
  | { kind: "working"; text: string }
  | { kind: "ready"; sheet: ModSheet }
  | { kind: "done"; text: string }
  | { kind: "failed"; text: string };

/** The page's one job: plan (the sheet), confirm, cancel. `generation`
 * moves whenever what is installed changes. */
export function useInstall() {
  const [step, setStep] = useState<Step | null>(null);
  const [generation, setGeneration] = useState(0);
  const working = step?.kind === "working";
  const onLine = useEffectEvent((line: string) => {
    if (working) setStep({ kind: "working", text: line });
  });
  useEffect(() => {
    let off: (() => void) | undefined;
    void onModsJob((line) => onLine(line)).then((f) => (off = f));
    return () => off?.();
  }, []);
  function plan(first: string, asking: Promise<ModSheet>) {
    setStep({ kind: "working", text: first });
    asking.then(
      (sheet) => setStep({ kind: "ready", sheet }),
      (err) => setStep({ kind: "failed", text: String(err) }),
    );
  }
  function confirm(sheet: ModSheet) {
    setStep({ kind: "working", text: "Working…" });
    modsConfirm(sheet.token).then(
      (text) => {
        setStep({ kind: "done", text });
        setGeneration((g) => g + 1);
      },
      (err) => setStep({ kind: "failed", text: String(err) }),
    );
  }
  function cancel(sheet: ModSheet) {
    setStep(null);
    void modsCancel(sheet.token).catch(() => {});
  }
  const busy = step?.kind === "working" || step?.kind === "ready";
  return {
    step,
    busy,
    generation,
    plan,
    confirm,
    cancel,
    dismiss: () => setStep(null),
  };
}

export type Install = ReturnType<typeof useInstall>;

/** One line under the game's name: working, waiting, done or failed. */
export function StepLine({ install }: { install: Install }) {
  const { step } = install;
  if (!step) return null;
  const text = step.kind === "ready" ? ON_SHEET : step.text;
  return (
    <p
      className={step.kind === "failed" ? "note" : "card-status"}
      role="status"
    >
      {text}
      {(step.kind === "done" || step.kind === "failed") && (
        <>
          {" "}
          <button type="button" className="quiet" onClick={install.dismiss}>
            OK
          </button>
        </>
      )}
    </p>
  );
}

/** A click from the pointer: keys and typed text never confirm a write. */
function pointer(e: MouseEvent) {
  return e.detail > 0;
}

/** The sheet: every file it adds, replaces, deletes or puts back. */
export function InstallSheet({ install }: { install: Install }) {
  const box = useRef<HTMLDivElement | null>(null);
  const sheet = install.step?.kind === "ready" ? install.step.sheet : null;
  useModalFocus(box, sheet !== null);
  if (!sheet) return null;
  return (
    <div className="first-run-backdrop">
      <div
        ref={box}
        className="card first-run"
        role="dialog"
        aria-modal="true"
        aria-labelledby="mods-sheet-title"
        onKeyDown={(e) => {
          if (e.key === "Escape") install.cancel(sheet);
        }}
      >
        <h2 id="mods-sheet-title">{sheet.title}</h2>
        {sheet.into && <p className="card-status">{sheet.into}</p>}
        {sheet.lists.map((l) => (
          <section key={l.title} aria-label={l.title}>
            <p className="card-status">
              <strong>{l.title}</strong>
            </p>
            {l.rows.length > 0 && (
              <ul className="first-run-list">
                {l.rows.map((r) => (
                  <li key={r}>
                    <code>{r}</code>
                  </li>
                ))}
              </ul>
            )}
          </section>
        ))}
        {sheet.notes.map((n) => (
          <p key={n} className="note">
            {n}
          </p>
        ))}
        <div className="first-run-foot">
          <button
            type="button"
            onClick={(e) => pointer(e) && install.confirm(sheet)}
          >
            {sheet.primary}
          </button>
          <button
            type="button"
            className="quiet"
            onClick={() => install.cancel(sheet)}
          >
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}

function FileActions({
  game,
  source,
  mod,
  file,
  install,
  sourceLabel,
  reload,
}: {
  game: string;
  source: string;
  mod: ModRow;
  file: ModFileRow;
  install: Install;
  sourceLabel: string;
  reload: () => void;
}) {
  const o = file.offer;
  const start = () =>
    install.plan(
      `Getting ${mod.name} ready…`,
      modsPlanInstall(game, source, mod.id, mod.name, file.id),
    );
  switch (o.kind) {
    case "install":
      return install.busy ? null : (
        <button type="button" onClick={start}>
          Install
        </button>
      );
    case "update":
      return (
        <>
          {!install.busy && (
            <button type="button" onClick={start}>
              Update
            </button>
          )}
          <span className="card-status">{o.was} installed</span>
        </>
      );
    case "installed":
      return <span className="card-status">Installed</span>;
    case "steam":
      return (
        <button
          type="button"
          className="quiet"
          onClick={() => void openUrl(o.url).catch(() => {})}
        >
          Subscribe in Steam
        </button>
      );
    case "browser":
      return (
        <>
          <button
            type="button"
            className="quiet"
            onClick={() => {
              void modsWaitForBrowser(game, source, mod.id, file.id).then(
                reload,
                () => {},
              );
              void openUrl(o.url).catch(() => {});
            }}
          >
            Download on {sourceLabel}
          </button>
          {file.waiting && !install.busy && (
            <button
              type="button"
              onClick={() =>
                install.plan(
                  `Getting ${mod.name} ready…`,
                  modsPickupDownload(game, source, mod.id, mod.name, file.id),
                )
              }
            >
              I downloaded it
            </button>
          )}
        </>
      );
    case "blocked":
      return null;
  }
}

/** The open mod's drawer: who made it, its page, its files with the one way
 * to get each, what it needs, and its description. */
export function ModDrawerPanel({
  game,
  source,
  sourceLabel,
  mod,
  offline,
  install,
  onClose,
}: {
  game: ModGame;
  source: string;
  sourceLabel: string;
  mod: ModRow;
  offline: boolean;
  install: Install;
  onClose: () => void;
}) {
  const [got, setGot] = useState<{ key: string; drawer: ModDrawer } | null>(
    null,
  );
  const [ask, setAsk] = useState(0);
  const key = `${game.id}|${source}|${mod.id}|${install.generation}|${ask}`;
  useEffect(() => {
    let live = true;
    modsFiles(game.id, source, mod.id, mod.summary).then(
      (drawer) => live && setGot({ key, drawer }),
      (err) =>
        live &&
        setGot({
          key,
          drawer: {
            files: [],
            blocked: null,
            needs: [],
            about: mod.summary,
            note: String(err),
          },
        }),
    );
    return () => {
      live = false;
    };
  }, [game.id, source, mod.id, mod.summary, key]);
  const drawer = got?.key === key ? got.drawer : null;
  return (
    <aside className="card mods-drawer" aria-label={`${mod.name} details`}>
      <div className="mods-head">
        <h3>{mod.name}</h3>
        <button type="button" className="quiet" onClick={onClose}>
          Close
        </button>
      </div>
      <p className="card-status">
        by {mod.author} on {sourceLabel}
      </p>
      {mod.pageUrl && (
        <button
          type="button"
          className="quiet"
          onClick={() => void openUrl(mod.pageUrl).catch(() => {})}
        >
          Open on {sourceLabel}
        </button>
      )}
      {offline ? (
        <p className="card-status">Offline only is on.</p>
      ) : (
        <>
          {drawer === null && <p className="empty">Looking…</p>}
          {drawer?.note && (
            <p className="card-status">
              {drawer.note}{" "}
              <button
                type="button"
                className="quiet"
                onClick={() => setAsk((n) => n + 1)}
              >
                Retry
              </button>
            </p>
          )}
          {drawer && drawer.files.length > 0 && (
            <section aria-label="Files">
              <p className="card-status">
                <strong>FILES</strong>
              </p>
              {drawer.blocked && <p className="note">{drawer.blocked}</p>}
              <ul className="mods-list">
                {drawer.files.map((f) => (
                  <li key={f.id}>
                    <strong>{f.name}</strong>
                    <p className="card-status">{f.line}</p>
                    <div className="mods-row-actions">
                      <FileActions
                        game={game.id}
                        source={source}
                        mod={mod}
                        file={f}
                        install={install}
                        sourceLabel={sourceLabel}
                        reload={() => setAsk((n) => n + 1)}
                      />
                    </div>
                  </li>
                ))}
              </ul>
            </section>
          )}
          {drawer && drawer.needs.length > 0 && (
            <section aria-label="Needs">
              <p className="card-status">
                <strong>NEEDS</strong>
              </p>
              <p>{drawer.needs.join(", ")}</p>
            </section>
          )}
          {drawer?.about && (
            <section aria-label="About">
              <p className="card-status">
                <strong>ABOUT</strong>
              </p>
              <p className="mods-summary">{drawer.about}</p>
            </section>
          )}
        </>
      )}
    </aside>
  );
}

/** The Installed tab: what the app put in, with Uninstall, then what else
 * the game has (Workshop items, addons), read only. */
export function InstalledTab({
  game,
  install,
}: {
  game: ModGame;
  install: Install;
}) {
  const [got, setGot] = useState<{ key: string; view: ModInstalled } | null>(
    null,
  );
  const [ask, setAsk] = useState(0);
  const [note, setNote] = useState<string | null>(null);
  const key = `${game.id}|${install.generation}|${ask}`;
  useEffect(() => {
    let live = true;
    modsInstalled(game.id).then(
      (view) => live && setGot({ key, view }),
      (err) => live && setNote(String(err)),
    );
    return () => {
      live = false;
    };
  }, [game.id, key]);
  const view = got?.key === key ? got.view : null;
  if (note) return <p className="note">{note}</p>;
  if (!view) return <p className="empty">Looking…</p>;
  const count = view.ours.length + view.others.length;
  return (
    <section aria-label="Installed">
      {view.recovered && <p className="card-status">{view.recovered}</p>}
      <div className="mods-head">
        <p className="card-status">{count} on this PC</p>
        <button
          type="button"
          className="quiet"
          onClick={() => setAsk((n) => n + 1)}
        >
          Refresh
        </button>
      </div>
      {count === 0 && <p className="empty">{NO_INSTALLED}</p>}
      <ul className="mods-list" aria-label="Installed mods">
        {view.ours.map((m) => (
          <li key={m.key} className="card mods-head">
            <div className="mods-row-body">
              <h3>{m.name}</h3>
              <p className="card-status">
                {m.version}, installed by {PRODUCT_NAME}
              </p>
              {m.offlineOnly && <p className="note">{m.offlineOnly}</p>}
            </div>
            <div className="mods-row-actions">
              {!install.busy && (
                <button
                  type="button"
                  onClick={() =>
                    install.plan(
                      `Checking ${m.name}…`,
                      modsPlanUninstall(game.id, m.key),
                    )
                  }
                >
                  Uninstall
                </button>
              )}
            </div>
          </li>
        ))}
        {view.others.map((r) => (
          <li key={r.name} className="card mods-head">
            <div className="mods-row-body">
              <h3>{r.name}</h3>
              <p className="card-status">{r.detail}</p>
            </div>
            {r.link && (
              <div className="mods-row-actions">
                <button
                  type="button"
                  className="quiet"
                  onClick={() => void openUrl(r.link ?? "").catch(() => {})}
                >
                  Open
                </button>
              </div>
            )}
          </li>
        ))}
      </ul>
    </section>
  );
}

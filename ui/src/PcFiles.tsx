import { useEffect, useState } from "react";
import {
  addPcPrinter,
  getPcFiles,
  pcNetCsv,
  pcResumeRows,
  pcSpeedTest,
  pcTidyReview,
  pcTidyTrash,
  removePcPrinter,
  setPcFilesSwitch,
  setPcFolders,
  setPcModelsFolder,
  setPcPrinterOn,
  type FilesHelpers,
  type FilesKey,
  type ResumeRow,
  type TidyRow,
} from "./pcFilesApi";
import { Toggle } from "./Toggle";

export const TIDY_NOTE =
  "Once a week, looks at the file names, sizes and ages directly in Downloads and suggests what could go: installers for apps you already have, big old disk images and archives, archives already unpacked, exact duplicates. Nothing moves until you pick files and approve the list, and then they go to the Trash. Off by default.";
export const MODEL_NOTE =
  "When an .stl, .3mf, .obj, .step or .stp file lands in Downloads, post a card with its size and the next steps. Only names with those extensions are looked at. Off by default.";
export const BACKUP_NOTE =
  "A gentle reminder, at most once a day, when commits aren't pushed, an important folder hasn't been backed up, or a backup tool hasn't run lately. Reads git metadata and file times only. Off by default.";
export const RESUME_NOTE =
  "Pick up where you left off: reads git metadata (branch, changed files, ahead and behind) for the repos in the code folders you pick. Never diffs or file contents. Off by default.";
export const NET_NOTE =
  "About once a minute, checks that the internet answers (a quick connection to two public DNS servers) and logs drops and slow minutes on this computer. Export the log for your internet provider any time. Off by default.";
export const PRINT_NOTE =
  "Poll printers on your network (Moonraker/Klipper or OctoPrint) for status only, and get a card when a print finishes, fails, pauses or runs out of filament. Addresses stay on this PC. Off by default.";
export const CAMERA_NOTE =
  "When a printer shares a webcam and an alert fires, attach one snapshot. Off by default.";

/** Settings → Your PC → PC helpers, part two: files, system and printers. */
export function PcFiles() {
  const [page, setPage] = useState<FilesHelpers | null>(null);
  const [note, setNote] = useState<string | null>(null);

  useEffect(() => {
    getPcFiles().then(setPage, (err) => setNote(String(err)));
  }, []);

  async function save(job: Promise<FilesHelpers>) {
    try {
      setPage(await job);
      setNote("Saved.");
      return true;
    } catch (err) {
      setNote(String(err));
      return false;
    }
  }

  if (!page) return <p className="note">{note ?? "Loading…"}</p>;
  const flip = (key: FilesKey) => (on: boolean) => void save(setPcFilesSwitch(key, on));

  return (
    <div className="behavior pc-files" role="group" aria-label="Files, system and printers">
      <Toggle checked={page.downloadsTidy} onChange={flip("downloadsTidy")}>
        Suggest a Downloads cleanup
      </Toggle>
      <p className="note">{TIDY_NOTE}</p>
      {page.downloadsTidy && <TidyReview onNote={setNote} />}

      <Toggle checked={page.modelDrop} onChange={flip("modelDrop")}>
        Offer next steps for new 3D models
      </Toggle>
      <p className="note">{MODEL_NOTE}</p>
      {page.modelDrop && (
        <AddRow
          label="Models folder"
          placeholder="Default: Models in your home folder"
          initial={page.modelsFolder}
          action="Save folder"
          onAdd={(folder) => save(setPcModelsFolder(folder))}
        />
      )}

      <Toggle checked={page.backupNudge} onChange={flip("backupNudge")}>
        Remind me to back up and push
      </Toggle>
      <p className="note">{BACKUP_NOTE}</p>
      {page.backupNudge && (
        <FolderList
          label="Important folders"
          folders={page.importantFolders}
          onSave={(list) => save(setPcFolders("importantFolders", list))}
        />
      )}

      <Toggle checked={page.resumeCode} onChange={flip("resumeCode")}>
        Pick up where I left off in code
      </Toggle>
      <p className="note">{RESUME_NOTE}</p>
      {page.resumeCode && (
        <>
          <FolderList
            label="Code folders"
            folders={page.codeFolders}
            onSave={(list) => save(setPcFolders("codeFolders", list))}
          />
          <ResumeList folders={page.codeFolders} onNote={setNote} />
        </>
      )}

      <Toggle checked={page.netLog} onChange={flip("netLog")}>
        Log internet drops
      </Toggle>
      <p className="note">{NET_NOTE}</p>
      {page.netLog && <NetLog page={page} onNote={setNote} onSaved={() => void getPcFiles().then(setPage)} />}

      <Toggle checked={page.printAlerts} onChange={flip("printAlerts")}>
        Watch 3D printers on my network
      </Toggle>
      <p className="note">{PRINT_NOTE}</p>
      {page.printAlerts && (
        <>
          <Toggle checked={page.printCamera} onChange={flip("printCamera")}>
            Include a camera snapshot
          </Toggle>
          <p className="note">{CAMERA_NOTE}</p>
        </>
      )}
      {(page.printAlerts || page.printers.length > 0) && (
        <>
          <ul className="list" aria-label="Saved printers">
            {page.printers.map((p) => (
              <li key={p.key}>
                <span>
                  {p.name} ({p.kind})
                </span>{" "}
                <button
                  type="button"
                  aria-pressed={p.enabled}
                  aria-label={`Watch ${p.name}`}
                  onClick={() => void save(setPcPrinterOn(p.key, !p.enabled))}
                >
                  {p.enabled ? "Watching" : "Paused"}
                </button>{" "}
                <button
                  type="button"
                  aria-label={`Remove ${p.name}`}
                  onClick={() => void save(removePcPrinter(p.key))}
                >
                  Remove
                </button>
              </li>
            ))}
          </ul>
          <AddRow
            label="Printer address"
            placeholder="192.168.1.50:7125"
            initial=""
            action="Add printer"
            clearOnAdd
            onAdd={(endpoint) => save(addPcPrinter(endpoint))}
          />
        </>
      )}
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
    </div>
  );
}

/** One text box and a button; the box clears after a good add when asked. */
function AddRow({
  label,
  placeholder,
  initial,
  action,
  clearOnAdd = false,
  onAdd,
}: {
  label: string;
  placeholder: string;
  initial: string;
  action: string;
  clearOnAdd?: boolean;
  onAdd: (text: string) => Promise<boolean>;
}) {
  const [text, setText] = useState(initial);
  return (
    <div className="card-buttons">
      <input aria-label={label} placeholder={placeholder} value={text} onChange={(e) => setText(e.target.value)} />
      <button
        type="button"
        disabled={!text.trim() && clearOnAdd}
        onClick={() =>
          void onAdd(text).then((ok) => {
            if (ok && clearOnAdd) setText("");
          })
        }
      >
        {action}
      </button>
    </div>
  );
}

function FolderList({
  label,
  folders,
  onSave,
}: {
  label: string;
  folders: string[];
  onSave: (list: string[]) => Promise<boolean>;
}) {
  return (
    <>
      {folders.length === 0 ? (
        <p className="note">No {label.toLowerCase()} yet.</p>
      ) : (
        <ul className="list" aria-label={label}>
          {folders.map((f) => (
            <li key={f}>
              <span>{f}</span>{" "}
              <button type="button" aria-label={`Remove ${f}`} onClick={() => void onSave(folders.filter((x) => x !== f))}>
                Remove
              </button>
            </li>
          ))}
        </ul>
      )}
      <AddRow
        label={`Add to ${label.toLowerCase()}`}
        placeholder="Full folder path"
        initial=""
        action="Add folder"
        clearOnAdd
        onAdd={(path) => onSave([...folders, path])}
      />
    </>
  );
}

/** The cleanup review: pick files, then approve the exact list. */
function TidyReview({ onNote }: { onNote: (note: string | null) => void }) {
  const [rows, setRows] = useState<TidyRow[] | null>(null);
  const [picked, setPicked] = useState<string[]>([]);
  const [asking, setAsking] = useState(false);

  async function review() {
    try {
      const found = await pcTidyReview();
      setRows(found);
      setPicked([]);
      setAsking(false);
      onNote(found.length === 0 ? "Nothing in Downloads looks ready to go." : null);
    } catch (err) {
      onNote(String(err));
    }
  }

  async function approve() {
    try {
      const done = await pcTidyTrash(picked);
      onNote(done.note);
      setRows(null);
      setAsking(false);
    } catch (err) {
      onNote(String(err));
      setAsking(false);
    }
  }

  const toggle = (name: string) =>
    setPicked((p) => (p.includes(name) ? p.filter((n) => n !== name) : [...p, name]));
  const bytes = (rows ?? []).filter((r) => picked.includes(r.name));

  return (
    <>
      <button type="button" onClick={() => void review()}>
        Review Downloads cleanup
      </button>
      {rows && rows.length > 0 && (
        <>
          <ul className="list" aria-label="Cleanup suggestions">
            {rows.map((r) => (
              <li key={r.name}>
                <button
                  type="button"
                  className="chip"
                  aria-pressed={picked.includes(r.name)}
                  onClick={() => {
                    setAsking(false);
                    toggle(r.name);
                  }}
                >
                  {r.name} · {r.size}
                </button>{" "}
                <span className="note">
                  {r.kind}: {r.why}
                </span>
              </li>
            ))}
          </ul>
          {!asking ? (
            <button type="button" disabled={picked.length === 0} onClick={() => setAsking(true)}>
              Move picked files to the Trash…
            </button>
          ) : (
            <div role="alertdialog" aria-label="Approve the list" className="card-buttons">
              <p className="note">
                Move {bytes.length} {bytes.length === 1 ? "file" : "files"} to the Trash: {bytes.map((r) => r.name).join(", ")}.
                Nothing is deleted for good.
              </p>
              <button type="button" onClick={() => void approve()}>
                Move to Trash
              </button>
              <button type="button" onClick={() => setAsking(false)}>
                Cancel
              </button>
            </div>
          )}
        </>
      )}
    </>
  );
}

function ResumeList({ folders, onNote }: { folders: string[]; onNote: (note: string | null) => void }) {
  const [rows, setRows] = useState<ResumeRow[] | null>(null);
  return (
    <>
      <button
        type="button"
        disabled={folders.length === 0}
        onClick={() =>
          void pcResumeRows().then(
            (r) => {
              setRows(r);
              onNote(r.length === 0 ? "Nothing waiting in those folders." : null);
            },
            (err) => onNote(String(err)),
          )
        }
      >
        Look at my repos now
      </button>
      {rows && rows.length > 0 && (
        <ul className="list" aria-label="Where you left off">
          {rows.map((r) => (
            <li key={r.name}>
              <strong>{r.name}</strong> <span className="note">{r.line}</span>
              {r.staleBranches > 0 && (
                <span className="note">
                  {" "}
                  · {r.staleBranches} merged {r.staleBranches === 1 ? "branch" : "branches"} to tidy
                </span>
              )}
            </li>
          ))}
        </ul>
      )}
    </>
  );
}

function NetLog({
  page,
  onNote,
  onSaved,
}: {
  page: FilesHelpers;
  onNote: (note: string | null) => void;
  onSaved: () => void;
}) {
  const [testing, setTesting] = useState(false);
  const worst = Math.max(...page.netByHour, 0);
  return (
    <>
      {page.netDown && <p className="note">The internet is down right now.</p>}
      <p className="note">
        {page.netDrops === 0 ? "No drops logged yet." : `${page.netDrops} drops in the last 30 days.`}
        {page.netLastSpeedMbps !== null && ` Last speed test: ${page.netLastSpeedMbps} Mbit/s.`}
      </p>
      {worst > 0 && (
        <p className="note">
          Most drops: {page.netByHour.map((n, h) => ({ n, h })).filter((x) => x.n === worst).map((x) => `${x.h}:00`).join(", ")}
        </p>
      )}
      <div className="card-buttons">
        <button
          type="button"
          disabled={testing}
          onClick={() => {
            setTesting(true);
            onNote("Testing download speed…");
            void pcSpeedTest().then(
              (mbps) => {
                setTesting(false);
                onNote(`Download speed: ${mbps} Mbit/s.`);
                onSaved();
              },
              (err) => {
                setTesting(false);
                onNote(String(err));
              },
            );
          }}
        >
          Test speed now
        </button>
        <button
          type="button"
          onClick={() =>
            void pcNetCsv().then(
              (path) => onNote(`Saved the log to ${path}.`),
              (err) => onNote(String(err)),
            )
          }
        >
          Export CSV
        </button>
      </div>
    </>
  );
}

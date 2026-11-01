// Composer attachments (LO-17i): the + button, a paste or a drop stages a
// file on the desktop (src-tauri/src/attach.rs); chips above the composer show
// what goes with the next message. Nothing is sent until the person sends.
import { useEffect, useEffectEvent, useState, type ClipboardEvent, type DragEvent } from "react";
import {
  onFileDrop,
  pickAttachments,
  removeAttachment,
  stageAttachment,
  stageAttachmentPaths,
  type StageOutcome,
  type Staged,
} from "./api";
import { PRODUCT_NAME } from "./brand";
import { inShell } from "./transport";

/** The cabin's limit (`grokhub_core::attach::PENDING_ATTACH_MAX`). */
export const ATTACH_MAX = 10;
/** The biggest file the desktop takes (`IMAGE_FILE_CAP`), checked before reading. */
export const ATTACH_BYTES_MAX = 8 * 1024 * 1024;
/** The web app's calls stop at 1 MB, and base64 grows a file by a third. */
export const REMOTE_ATTACH_BYTES_MAX = 700 * 1024;
export const TOO_MANY = "too many attachments";
export const TOO_LARGE = "file is too large";
export const REMOTE_TOO_BIG = `is over 700 KB, too big for the web app. Attach it in ${PRODUCT_NAME} on your computer.`;

/** Pictures the desktop can't read itself (GIF, WebP, BMP, SVG) are redrawn
 * as PNG here first. Where the browser can't, the file goes as it is and the
 * desktop says why it won't take it. */
async function asSendable(file: File): Promise<{ name: string; blob: Blob }> {
  const plain = file.type === "image/png" || file.type === "image/jpeg";
  if (!file.type.startsWith("image/") || plain || typeof createImageBitmap !== "function") {
    return { name: file.name, blob: file };
  }
  try {
    const bitmap = await createImageBitmap(file);
    const canvas = document.createElement("canvas");
    canvas.width = bitmap.width;
    canvas.height = bitmap.height;
    canvas.getContext("2d")?.drawImage(bitmap, 0, 0);
    const png = await new Promise<Blob | null>((done) => canvas.toBlob(done, "image/png"));
    if (!png) return { name: file.name, blob: file };
    const base = file.name.replace(/\.[^.]*$/, "") || "picture";
    return { name: `${base}.png`, blob: png };
  } catch {
    return { name: file.name, blob: file };
  }
}

async function base64(blob: Blob): Promise<string> {
  const bytes = new Uint8Array(await blob.arrayBuffer());
  let binary = "";
  for (let i = 0; i < bytes.length; i += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  return btoa(binary);
}

/** A pasted image often has no useful name. */
function fileName(file: File): string {
  if (file.name) return file.name;
  return file.type === "image/jpeg" ? "pasted.jpg" : "pasted.png";
}

export interface Attachments {
  files: Staged[];
  /** Why the last file didn't attach, if it didn't. */
  note: string | null;
  add: (files: File[]) => Promise<void>;
  pick: () => Promise<void>;
  remove: (path: string) => void;
  /** After a send: the chips go, the copies stay with the chat. */
  clear: () => void;
  onPaste: (event: ClipboardEvent) => void;
  onDragOver: (event: DragEvent) => void;
  onDrop: (event: DragEvent) => void;
}

/** The composer's waiting files for one conversation. `active`: the Chat tab
 * shows, so a drop on the window is for it. */
export function useAttachments(
  conversationId: string,
  { remote, active }: { remote: boolean; active: boolean },
): Attachments {
  const [files, setFiles] = useState<Staged[]>([]);
  const [note, setNote] = useState<string | null>(null);
  // A new conversation starts with no files waiting.
  const [forId, setForId] = useState(conversationId);
  if (forId !== conversationId) {
    setForId(conversationId);
    setFiles([]);
    setNote(null);
  }

  function take(outcomes: StageOutcome[]) {
    const staged = outcomes.flatMap((o) => (o.staged ? [o.staged] : []));
    const errors = outcomes.flatMap((o) => (o.error ? [o.error] : []));
    if (staged.length) setFiles((now) => [...now, ...staged]);
    setNote(errors.length ? errors.join(" ") : null);
  }

  async function add(list: File[]) {
    const outcomes: StageOutcome[] = [];
    let room = ATTACH_MAX - files.length;
    for (const file of list) {
      const name = fileName(file);
      if (room <= 0) {
        outcomes.push({ staged: null, error: `${name}: ${TOO_MANY}` });
        continue;
      }
      if (remote && file.size > REMOTE_ATTACH_BYTES_MAX) {
        outcomes.push({ staged: null, error: `${name} ${REMOTE_TOO_BIG}` });
        continue;
      }
      if (file.size > ATTACH_BYTES_MAX) {
        outcomes.push({ staged: null, error: `${name}: ${TOO_LARGE}` });
        continue;
      }
      try {
        const ready = await asSendable(file);
        const staged = await stageAttachment(conversationId, ready.name, await base64(ready.blob));
        outcomes.push({ staged, error: null });
        room -= 1;
      } catch (err) {
        outcomes.push({ staged: null, error: String(err) });
      }
    }
    take(outcomes);
  }

  async function addPaths(paths: string[]) {
    try {
      take(await stageAttachmentPaths(conversationId, paths));
    } catch (err) {
      setNote(String(err));
    }
  }

  // The desktop window hands drops over as paths.
  const dropped = useEffectEvent((paths: string[]) => {
    if (active) void addPaths(paths);
  });
  useEffect(() => {
    let stop: (() => void) | undefined;
    let gone = false;
    void onFileDrop((paths) => dropped(paths)).then((unlisten) => {
      if (gone) unlisten();
      else stop = unlisten;
    });
    return () => {
      gone = true;
      stop?.();
    };
  }, []);

  async function pick() {
    try {
      take(await pickAttachments(conversationId));
    } catch (err) {
      setNote(String(err));
    }
  }

  function remove(path: string) {
    setFiles((now) => now.filter((f) => f.path !== path));
    setNote(null);
    void removeAttachment(conversationId, path).catch(() => {});
  }

  function onPaste(event: ClipboardEvent) {
    const list = Array.from(event.clipboardData?.files ?? []);
    if (list.length === 0) return;
    event.preventDefault();
    void add(list);
  }

  function onDragOver(event: DragEvent) {
    if (event.dataTransfer?.types?.includes("Files")) event.preventDefault();
  }

  function onDrop(event: DragEvent) {
    const list = Array.from(event.dataTransfer?.files ?? []);
    if (list.length === 0) return;
    event.preventDefault();
    // The desktop window gets the same drop as paths (onFileDrop).
    if (!inShell()) void add(list);
  }

  return {
    files,
    note,
    add,
    pick,
    remove,
    clear: () => {
      setFiles([]);
      setNote(null);
    },
    onPaste,
    onDragOver,
    onDrop,
  };
}

/** The chips above the composer, each with its ×, and the last refusal. */
export function AttachChips({
  files,
  note,
  onRemove,
  busy,
}: {
  files: Staged[];
  note: string | null;
  onRemove: (path: string) => void;
  busy: boolean;
}) {
  if (files.length === 0 && !note) return null;
  return (
    <div className="attach-chips" role="group" aria-label="Attachments">
      {files.map((f) => (
        <span key={f.path} className={f.status ? "chip attach warn" : "chip attach"} title={f.name}>
          <span>{f.label}</span>
          {f.status && <span className="attach-status">{f.status}</span>}
          <button
            type="button"
            className="link"
            aria-label={`Remove ${f.name}`}
            disabled={busy}
            onClick={() => onRemove(f.path)}
          >
            ×
          </button>
        </span>
      ))}
      {note && (
        <p className="note warn" role="status">
          {note}
        </p>
      )}
    </div>
  );
}

/** The composer's +. The web app takes pastes and drops only. */
export function AttachButton({ onPick, busy, full }: { onPick: () => void; busy: boolean; full: boolean }) {
  return (
    <button
      type="button"
      className="quiet attach-plus"
      aria-label="Attach files"
      title={full ? TOO_MANY : "Attach files"}
      disabled={busy || full}
      onClick={onPick}
    >
      +
    </button>
  );
}

import { useCallback, useEffect, useRef, useState } from "react";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { bracketMatching, HighlightStyle, indentOnInput, syntaxHighlighting } from "@codemirror/language";
import { tags } from "@lezer/highlight";
import { EditorState, type Extension } from "@codemirror/state";
import { drawSelection, EditorView, highlightActiveLine, keymap, lineNumbers } from "@codemirror/view";
import { codeFileRead, codeFilesList, codeFileWrite, type CodeFileListing, type CodeFileText } from "./api";
import { useCardFocus } from "./ui";

export const EDITOR_TITLE = "Editor";
export const EDITOR_EMPTY = "Pick a file on the left to open it.";
export const EDITOR_DISCARD = "Discard unsaved changes?";

/** Syntax colors from the app's own theme: keywords in the accent, strings
 * and numbers in steel, comments muted, everything else plain text. */
const HIGHLIGHT = HighlightStyle.define([
  { tag: [tags.keyword, tags.controlKeyword, tags.modifier, tags.definitionKeyword], color: "var(--accent)", fontWeight: "600" },
  { tag: [tags.string, tags.special(tags.string), tags.number, tags.bool, tags.regexp, tags.atom], color: "var(--steel)" },
  { tag: [tags.comment, tags.lineComment, tags.blockComment, tags.docComment], color: "var(--muted)" },
  { tag: [tags.heading, tags.strong], fontWeight: "600" },
  { tag: tags.link, textDecoration: "underline" },
  { tag: tags.invalid, color: "var(--danger-text)" },
]);

/** Syntax colors for a file, by its extension; each language loads only when
 * a file of its kind opens. Unknown kinds get plain text. */
export async function languageFor(path: string): Promise<Extension[]> {
  const ext = path.toLowerCase().split(".").pop() ?? "";
  if (ext === "rs") return [(await import("@codemirror/lang-rust")).rust()];
  if (ext === "json") return [(await import("@codemirror/lang-json")).json()];
  if (ext === "md" || ext === "markdown") return [(await import("@codemirror/lang-markdown")).markdown()];
  const js = ["js", "mjs", "cjs", "jsx"].includes(ext);
  const ts = ["ts", "mts", "cts", "tsx"].includes(ext);
  if (js || ts) {
    const { javascript } = await import("@codemirror/lang-javascript");
    return [javascript({ typescript: ts, jsx: ext.endsWith("x") })];
  }
  return [];
}

/** The folder above `path` (`""` is the project root). */
export function parentOf(path: string): string {
  const cut = path.lastIndexOf("/");
  return cut < 0 ? "" : path.slice(0, cut);
}

/** True for text whose every line ending is CRLF. The editor holds text with
 * LF only, so a save puts these back; a file with mixed endings is left as
 * the editor has it. */
export function usesCrlf(text: string): boolean {
  return text.includes("\r\n") && !/\r(?!\n)|(?<!\r)\n/.test(text);
}

/** The Code tab's editor: a folder list of the project and one open file.
 * `refresh` is a counter: a new value lists the open folder again. */
export default function CodeEditor({ refresh = 0 }: { refresh?: number }) {
  const [listing, setListing] = useState<CodeFileListing | null>(null);
  const [path, setPath] = useState<string | null>(null);
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [pending, setPending] = useState<string | null>(null);
  const host = useRef<HTMLDivElement | null>(null);
  const view = useRef<EditorView | null>(null);
  // The saved text as the editor holds it (LF line endings), and whether the
  // file on disk uses CRLF.
  const saved = useRef("");
  const crlf = useRef(false);
  // Counts files opened and files picked: a save or a read that lands after a
  // newer one must not touch the file on screen.
  const opened = useRef(0);
  const picked = useRef(0);
  // The folder on screen, so the list comes back to it.
  const folder = useRef("");
  const keep = useCardFocus<HTMLButtonElement>(pending);
  // The project folder the open file was read from. A save sends it back, and
  // the desktop refuses the save when the project folder has changed since.
  const root = useRef("");
  const saveRef = useRef<() => void>(() => {});

  const list = useCallback((next: CodeFileListing) => {
    folder.current = next.path;
    setListing(next);
  }, []);

  const browse = useCallback(
    async (to: string) => {
      try {
        list(await codeFilesList(to));
      } catch (err) {
        setNote(String(err));
      }
    },
    [list],
  );

  // Runs each time the tab shows and on Refresh: the folder that was open is
  // listed again, or the top folder when it is gone.
  useEffect(() => {
    const at = folder.current;
    codeFilesList(at)
      .catch((err) => {
        if (at === "") throw err;
        return codeFilesList("");
      })
      .then(list, (err) => setNote(String(err)));
  }, [list, refresh]);

  // When the Code page is hidden its effects pause: the open file's state is
  // kept (unsaved edits included) and the editor comes back as it was.
  const stash = useRef<EditorState | null>(null);
  useEffect(() => {
    if (stash.current && host.current && !view.current) {
      view.current = new EditorView({ state: stash.current, parent: host.current });
      stash.current = null;
    }
    return () => {
      if (view.current) {
        stash.current = view.current.state;
        view.current.destroy();
        view.current = null;
      }
    };
  }, []);

  /** The open file's text as the editor holds it, also while the page is hidden. */
  const docText = useCallback((): string | null => {
    const state = view.current?.state ?? stash.current;
    return state ? state.doc.toString() : null;
  }, []);

  const show = useCallback((got: CodeFileText, language: Extension[]) => {
    const { path: file, text } = got;
    view.current?.destroy();
    view.current = null;
    stash.current = null;
    opened.current += 1;
    root.current = got.root;
    crlf.current = usesCrlf(text);
    const state = EditorState.create({
      doc: text,
      extensions: [
        lineNumbers(),
        history(),
        drawSelection(),
        highlightActiveLine(),
        indentOnInput(),
        bracketMatching(),
        syntaxHighlighting(HIGHLIGHT),
        keymap.of([
          { key: "Mod-s", preventDefault: true, run: () => (saveRef.current(), true) },
          indentWithTab,
          ...defaultKeymap,
          ...historyKeymap,
        ]),
        EditorView.contentAttributes.of({ "aria-label": `Text of ${file}` }),
        EditorView.updateListener.of((u) => {
          if (u.docChanged) setDirty(u.state.doc.toString() !== saved.current);
        }),
        ...language,
      ],
    });
    // The editor joins lines with LF, whatever the file uses.
    saved.current = state.doc.toString();
    if (host.current) view.current = new EditorView({ state, parent: host.current });
    else stash.current = state;
    setPath(file);
    setDirty(false);
  }, []);

  /** `discard` is true when the person chose to throw the open file's edits away. */
  async function open(file: string, discard = false) {
    setPending(null);
    setNote(null);
    const pick = ++picked.current;
    try {
      const got = await codeFileRead(file);
      const language = await languageFor(got.path).catch(() => []);
      // A newer pick was made while this one was on its way.
      if (pick !== picked.current) return;
      // Edits typed while it was on its way are not thrown away unasked.
      const now = docText();
      if (!discard && now !== null && now !== saved.current) {
        setPending(file);
        return;
      }
      show(got, language);
    } catch (err) {
      if (pick === picked.current) setNote(String(err));
    }
  }

  function pick(file: string) {
    // The newest pick wins: an older one still on its way is dropped.
    picked.current += 1;
    if (file === path) return;
    if (dirty) setPending(file);
    else void open(file);
  }

  async function save() {
    const body = docText();
    if (body === null || path === null || saving) return;
    // Nothing changed: no write, so the file's time and the activity log stay as they are.
    if (body === saved.current) return;
    const file = path;
    const at = opened.current;
    const text = crlf.current ? body.replace(/\n/g, "\r\n") : body;
    setSaving(true);
    setNote(null);
    try {
      await codeFileWrite(file, text, root.current);
      // Another file may have been opened since; its unsaved mark is its own.
      if (at === opened.current) {
        saved.current = body;
        setDirty(docText() !== body);
      }
      setNote(`Saved ${file}.`);
    } catch (err) {
      setNote(String(err));
    } finally {
      setSaving(false);
    }
  }
  // The editor's Mod-s key reaches the newest save through this.
  useEffect(() => {
    saveRef.current = () => void save();
  });

  return (
    <div className="code-editor">
      <h3>{EDITOR_TITLE}</h3>
      <div className="code-split">
        <ul className="rows code-tree" aria-label="Project files">
          {listing && listing.path !== "" && (
            <li>
              <button type="button" className="code-file quiet" onClick={() => void browse(parentOf(listing.path))}>
                ../ <span className="note">(up from {listing.path})</span>
              </button>
            </li>
          )}
          {listing?.entries.map((e) => (
            <li key={e.path}>
              <button
                type="button"
                className="code-file quiet"
                aria-pressed={!e.dir && e.path === path}
                onClick={() => (e.dir ? void browse(e.path) : pick(e.path))}
              >
                {e.dir ? `${e.name}/` : e.name}
              </button>
            </li>
          ))}
          {listing?.more && <li className="note">This folder has more entries than this list shows.</li>}
        </ul>
        <div className="code-editor-pane">
          {path !== null && (
            <div className="code-editor-bar">
              <span className="code-editor-path">
                {path}
                {dirty && <span className="note"> · unsaved</span>}
              </span>
              <button type="button" onClick={() => void save()} disabled={!dirty || saving}>
                {saving ? "Saving…" : "Save"}
              </button>
            </div>
          )}
          {note && (
            <p className="note" role="status">
              {note}
            </p>
          )}
          {pending !== null && (
            <div className="card confirm-window" role="group" aria-label={EDITOR_DISCARD}>
              <p>
                {EDITOR_DISCARD} {path} has edits that aren't saved.
              </p>
              <div className="card-buttons">
                <button type="button" onClick={() => void open(pending, true)}>
                  Discard and open {pending}
                </button>
                <button ref={keep} type="button" className="quiet" onClick={() => setPending(null)}>
                  Keep editing
                </button>
              </div>
            </div>
          )}
          {path === null && <p className="empty">{EDITOR_EMPTY}</p>}
          <div ref={host} className="code-editor-host" hidden={path === null} />
        </div>
      </div>
    </div>
  );
}

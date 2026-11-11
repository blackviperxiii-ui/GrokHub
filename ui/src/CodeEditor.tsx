import { useCallback, useEffect, useRef, useState } from "react";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { bracketMatching, HighlightStyle, indentOnInput, syntaxHighlighting } from "@codemirror/language";
import { tags } from "@lezer/highlight";
import { EditorState, type Extension } from "@codemirror/state";
import { drawSelection, EditorView, highlightActiveLine, keymap, lineNumbers } from "@codemirror/view";
import { codeFileRead, codeFilesList, codeFileWrite, type CodeFileListing } from "./api";

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

/** The Code tab's editor: a folder list of the project and one open file. */
export default function CodeEditor() {
  const [listing, setListing] = useState<CodeFileListing | null>(null);
  const [path, setPath] = useState<string | null>(null);
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [pending, setPending] = useState<string | null>(null);
  const host = useRef<HTMLDivElement | null>(null);
  const view = useRef<EditorView | null>(null);
  const saved = useRef("");
  const saveRef = useRef<() => void>(() => {});

  const browse = useCallback(async (folder: string) => {
    try {
      setListing(await codeFilesList(folder));
    } catch (err) {
      setNote(String(err));
    }
  }, []);

  useEffect(() => {
    codeFilesList("").then(setListing, (err) => setNote(String(err)));
  }, []);

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

  const show = useCallback((file: string, text: string, language: Extension[]) => {
    view.current?.destroy();
    saved.current = text;
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
    if (host.current) view.current = new EditorView({ state, parent: host.current });
    setPath(file);
    setDirty(false);
  }, []);

  async function open(file: string) {
    setPending(null);
    setNote(null);
    try {
      const got = await codeFileRead(file);
      const language = await languageFor(got.path).catch(() => []);
      show(got.path, got.text, language);
    } catch (err) {
      setNote(String(err));
    }
  }

  function pick(file: string) {
    if (file === path) return;
    if (dirty) setPending(file);
    else void open(file);
  }

  async function save() {
    const v = view.current;
    if (!v || path === null || saving) return;
    const text = v.state.doc.toString();
    setSaving(true);
    setNote(null);
    try {
      await codeFileWrite(path, text);
      saved.current = text;
      setDirty(v.state.doc.toString() !== text);
      setNote(`Saved ${path}.`);
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
          {pending !== null && (
            <div className="card confirm-window" role="group" aria-label={EDITOR_DISCARD}>
              <p>
                {EDITOR_DISCARD} {path} has edits that aren't saved.
              </p>
              <div className="card-buttons">
                <button type="button" onClick={() => void open(pending)}>
                  Discard and open {pending}
                </button>
                <button type="button" className="quiet" onClick={() => setPending(null)}>
                  Keep editing
                </button>
              </div>
            </div>
          )}
          {path === null && <p className="empty">{EDITOR_EMPTY}</p>}
          <div ref={host} className="code-editor-host" hidden={path === null} />
        </div>
      </div>
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
    </div>
  );
}

import { useEffect, useRef, useState } from "react";
import { confirmSkillInstall, type SkillInstallPreview } from "./api";
import { useModalFocus } from "./ui";

/** Confirm dialog for a `loadout://skill/install` deep link. */
export function SkillInstallConfirm({
  preview,
  onDone,
  onCancel,
}: {
  preview: SkillInstallPreview;
  onDone: (note: string) => void;
  onCancel: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const cancel = useRef<HTMLButtonElement>(null);
  const box = useRef<HTMLDivElement>(null);
  const skills = preview.skills ?? [];
  useModalFocus(box);
  useEffect(() => {
    cancel.current?.focus();
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape" && !busy) onCancel();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [busy, onCancel]);

  async function install() {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      const done = await confirmSkillInstall(preview.identifier);
      onDone(done.note);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setBusy(false);
    }
  }

  return (
    <div
      className="fix-backdrop"
      onClick={() => {
        if (!busy) onCancel();
      }}
    >
      <div
        ref={box}
        className="card fix-window confirm-window"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="skill-install-title"
        onClick={(event) => event.stopPropagation()}
      >
        <h2 id="skill-install-title">Install “{preview.name}”?</h2>
        <p>
          <strong>Source</strong> {preview.source}
        </p>
        <p>
          <strong>Install to</strong> {preview.destination}
        </p>
        {preview.description ? <p className="note">{preview.description}</p> : null}
        {skills.length > 0 ? (
          <p className="note">Bundle skills: {skills.join(", ")}.</p>
        ) : null}
        <p className="note">
          Install runs the skill safety check. Quarantined skills stay off until
          cleaned up.
        </p>
        {error ? (
          <p className="note" role="alert">
            {error}
          </p>
        ) : null}
        <div className="card-buttons">
          <button type="button" disabled={busy} onClick={() => void install()}>
            {busy ? "Installing…" : "Install"}
          </button>
          <button
            ref={cancel}
            type="button"
            className="quiet"
            disabled={busy}
            onClick={onCancel}
          >
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}

import { useEffect, useState } from "react";
import { clearProfilePicture, getProfile, pickProfilePicture, setProfileName, type Profile } from "./api";
import { StatusLine, useNote } from "./ui";

export const NAME_HINT = "Shown on the rail. Leave this blank to use your Grok name.";
export const PICTURE_SET = "Saved in your settings on this PC.";
export const PICTURE_UNSET = "A local image, kept in your settings on this PC.";
export const REMOVE_HINT = "The rail goes back to your initial.";
/** The longest name kept (profile::NAME_MAX). */
const NAME_MAX = 64;

/** The picture, or the initial in a circle. */
export function Avatar({ profile, className }: { profile: Profile; className: string }) {
  return profile.picture ? (
    <img className={className} src={profile.picture} alt="" />
  ) : (
    <span className={className} aria-hidden="true">
      {profile.initial}
    </span>
  );
}

/** Settings → Account → Profile: your name and picture, shown on the rail.
 * `onProfile` gets the profile after each change. */
export function ProfileSettings({ onProfile }: { onProfile?: (p: Profile) => void }) {
  const [profile, setProfile] = useState<Profile | null>(null);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const { note, ok, fail, clear } = useNote();

  useEffect(() => {
    getProfile().then((p) => {
      setProfile(p);
      setDraft(p.name);
    }, fail);
  }, [fail]);

  function show(p: Profile) {
    setProfile(p);
    setDraft(p.name);
    onProfile?.(p);
  }

  async function step(change: () => Promise<Profile | null>, waiting?: string) {
    setBusy(true);
    if (waiting) ok(waiting);
    try {
      const p = await change();
      if (p) {
        show(p);
        ok("Saved");
      } else {
        clear();
      }
    } catch (err) {
      fail(err);
    } finally {
      setBusy(false);
    }
  }

  function saveName() {
    if (!profile || busy) return;
    const next = draft.trim();
    if (next === profile.name) {
      setDraft(next);
      return;
    }
    void step(() => setProfileName(next));
  }

  if (!profile) {
    return (
      <div className="settings-group" role="group" aria-label="Profile">
        <h2>Profile</h2>
        <StatusLine note={note} className="note" />
      </div>
    );
  }

  return (
    <div className="settings-group profile" role="group" aria-label="Profile">
      <h2>Profile</h2>
      <form
        className="profile-name"
        onSubmit={(e) => {
          e.preventDefault();
          saveName();
        }}
      >
        <label>
          Name
          <input
            aria-label="Name"
            maxLength={NAME_MAX}
            value={draft}
            placeholder={profile.shownName}
            onChange={(e) => setDraft(e.target.value)}
            onBlur={saveName}
          />
        </label>
      </form>
      <p className="note">{NAME_HINT}</p>
      <div className="profile-picture">
        <Avatar profile={profile} className="profile-avatar" />
        <div>
          <p>Profile picture</p>
          <p className="note">{profile.picture ? PICTURE_SET : PICTURE_UNSET}</p>
        </div>
        <button type="button" disabled={busy} onClick={() => void step(pickProfilePicture, "Choose a picture…")}>
          Choose
        </button>
      </div>
      {profile.picture && (
        <div className="profile-picture">
          <div>
            <p>Remove picture</p>
            <p className="note">{REMOVE_HINT}</p>
          </div>
          <button type="button" className="quiet" disabled={busy} onClick={() => void step(clearProfilePicture)}>
            Remove
          </button>
        </div>
      )}
      <StatusLine note={note} className="note" />
    </div>
  );
}

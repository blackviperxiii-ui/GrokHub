import { useEffect, useEffectEvent, useState } from "react";
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
 * `onProfile` gets the profile after each change and each read. A new
 * `signedIn` reads it again, since the Grok name is the fallback name.
 * `canPick`: this device has a file picker (phones don't). */
export function ProfileSettings({
  onProfile,
  signedIn,
  canPick,
}: {
  onProfile?: (p: Profile) => void;
  signedIn?: string;
  canPick: boolean;
}) {
  const [profile, setProfile] = useState<Profile | null>(null);
  const [draft, setDraft] = useState("");
  // The name and the picture save on their own, so one never blocks the other.
  const [savingName, setSavingName] = useState(false);
  const [savingPicture, setSavingPicture] = useState(false);
  const { note, ok, fail, clear } = useNote();

  const loaded = useEffectEvent((p: Profile) => {
    // A read again (after a Grok sign-in) keeps a name being typed.
    if (!profile || draft === profile.name) setDraft(p.name);
    setProfile(p);
    onProfile?.(p);
  });
  useEffect(() => {
    getProfile().then(loaded, fail);
  }, [fail, signedIn]);

  function show(p: Profile) {
    setProfile(p);
    onProfile?.(p);
  }

  async function saveName() {
    if (!profile || savingName) return;
    const sent = draft.trim();
    if (sent === profile.name) {
      setDraft(sent);
      return;
    }
    setSavingName(true);
    try {
      const p = await setProfileName(sent);
      show(p);
      setDraft(p.name);
      ok("Saved");
    } catch (err) {
      fail(err);
    } finally {
      setSavingName(false);
    }
  }

  async function picture(change: () => Promise<Profile | null>, waiting?: string) {
    setSavingPicture(true);
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
      setSavingPicture(false);
    }
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
          void saveName();
        }}
      >
        <label>
          Name
          <input
            aria-label="Name"
            maxLength={NAME_MAX}
            value={draft}
            placeholder={profile.shownName}
            // Typing waits for a save in flight, so nothing typed meanwhile is lost.
            readOnly={savingName}
            onChange={(e) => setDraft(e.target.value)}
            onBlur={() => void saveName()}
          />
        </label>
      </form>
      <p className="note">{NAME_HINT}</p>
      <div className="profile-picture">
        <Avatar profile={profile} className="profile-avatar" />
        {canPick && (
          <>
            <div>
              <p>Profile picture</p>
              <p className="note">{profile.picture ? PICTURE_SET : PICTURE_UNSET}</p>
            </div>
            <button
              type="button"
              disabled={savingPicture}
              onClick={() => void picture(pickProfilePicture, "Choose a picture…")}
            >
              Choose
            </button>
          </>
        )}
      </div>
      {profile.picture && (
        <div className="profile-picture">
          <div>
            <p>Remove picture</p>
            <p className="note">{REMOVE_HINT}</p>
          </div>
          <button
            type="button"
            className="quiet"
            disabled={savingPicture}
            onClick={() => void picture(clearProfilePicture)}
          >
            Remove
          </button>
        </div>
      )}
      <StatusLine note={note} className="note" />
    </div>
  );
}

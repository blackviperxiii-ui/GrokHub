import { useEffect, useState, type FormEvent, type MouseEvent } from "react";
import {
  getMeetings,
  meetingsAct,
  type MeetingHit,
  type MeetingRow,
  type MeetingsPage,
} from "./api";
import { StatusLine, useNote } from "./ui";

export const MEETINGS_EMPTY =
  "No past meetings yet. Allow Join and Record, start a meeting, follow notes, and use Talk.";

export const JOIN_HINT =
  "Off until you allow it. Lets you start a meeting session in Loadout. Nothing joins Meet, Zoom, or Teams for you.";

export const RECORD_HINT =
  "Off until you allow it. Keeps a transcript of notes, Talk turns, and speech lines you save. Explicit consent before any recording.";

/** Allow counts only from the pointer: Enter or Space on a focused Allow does nothing. */
function pointer(e: MouseEvent) {
  return e.detail > 0;
}

function kindLabel(kind: string): string {
  switch (kind) {
    case "note":
      return "Note";
    case "talk":
      return "Talk";
    case "speech":
      return "Speech";
    default:
      return kind;
  }
}

function LineList({ lines }: { lines: MeetingRow["transcript"] }) {
  if (lines.length === 0) return <p className="empty">No transcript lines yet.</p>;
  return (
    <ul className="rows">
      {lines.map((line) => (
        <li key={line.id}>
          <strong>{kindLabel(line.kind)}</strong>
          {line.speaker ? ` · ${line.speaker}` : ""}
          {": "}
          {line.text}
        </li>
      ))}
    </ul>
  );
}

function HitList({ hits }: { hits: MeetingHit[] }) {
  if (hits.length === 0) return null;
  return (
    <ul className="rows" aria-label="Search hits">
      {hits.map((hit) => (
        <li key={`${hit.meetingId}-${hit.lineId}`}>
          <strong>{hit.meetingTitle}</strong>
          {" · "}
          {kindLabel(hit.kind)}
          {hit.speaker ? ` · ${hit.speaker}` : ""}
          {": "}
          {hit.text}
        </li>
      ))}
    </ul>
  );
}

/** Live meetings, in-meeting Talk, followable notes, and transcript search.
 * Join and Record stay off until a pointer Allow. */
export function Meetings() {
  const [page, setPage] = useState<MeetingsPage | null>(null);
  const [title, setTitle] = useState("");
  const [notes, setNotes] = useState("");
  const [talk, setTalk] = useState("");
  const [speechWho, setSpeechWho] = useState("");
  const [speechText, setSpeechText] = useState("");
  const [query, setQuery] = useState("");
  const { note, ok, fail } = useNote();
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    getMeetings("").then((got) => {
      setPage(got);
      setNotes(got.live?.liveNotes ?? "");
      setQuery(got.query);
    }, fail);
  }, [fail]);

  async function run(work: () => Promise<MeetingsPage>, done?: string) {
    setBusy(true);
    try {
      const got = await work();
      setPage(got);
      setNotes(got.live?.liveNotes ?? notes);
      setQuery(got.query || query);
      ok(done ?? null);
      return true;
    } catch (err) {
      fail(err);
      await getMeetings(query).then(setPage, () => {});
      return false;
    } finally {
      setBusy(false);
    }
  }

  async function allowJoin(e: MouseEvent, on: boolean) {
    if (!pointer(e) || busy) return;
    await run(() => meetingsAct({ kind: "setJoin", on }, query), on ? "Join allowed." : "Join revoked.");
  }

  async function allowRecord(e: MouseEvent, on: boolean) {
    if (!pointer(e) || busy) return;
    await run(
      () => meetingsAct({ kind: "setRecord", on }, query),
      on ? "Record allowed." : "Record revoked.",
    );
  }

  async function start(event: FormEvent) {
    event.preventDefault();
    if (busy) return;
    if (await run(() => meetingsAct({ kind: "start", title }, query), "Meeting live.")) {
      setTitle("");
      setNotes("");
    }
  }

  async function saveNotes(event: FormEvent) {
    event.preventDefault();
    if (busy) return;
    await run(() => meetingsAct({ kind: "followNotes", notes }, query), "Notes updated.");
  }

  async function sendTalk(event: FormEvent) {
    event.preventDefault();
    if (!talk.trim() || busy) return;
    if (await run(() => meetingsAct({ kind: "talk", text: talk }, query), "Talk saved.")) {
      setTalk("");
    }
  }

  async function addSpeech(event: FormEvent) {
    event.preventDefault();
    if (!speechText.trim() || busy) return;
    if (
      await run(
        () => meetingsAct({ kind: "speech", speaker: speechWho, text: speechText }, query),
        "Speech saved.",
      )
    ) {
      setSpeechText("");
    }
  }

  async function endMeeting() {
    if (busy) return;
    await run(() => meetingsAct({ kind: "end" }, query), "Meeting ended.");
  }

  async function search(event: FormEvent) {
    event.preventDefault();
    if (busy) return;
    await run(() => meetingsAct({ kind: "search", query }, query));
  }

  return (
    <section className="meetings journal" aria-label="Meetings">
      <h2>Meetings</h2>
      <p className="hint">
        Follow live notes and use in-meeting Talk during a call. Transcripts stay on this computer
        and are searchable after. Join and Record need an explicit Allow — nothing records or joins
        until you do.
      </p>

      <div className="rows" role="group" aria-label="Privacy and consent">
        <div className="consent-row" style={{ display: "flex", gap: 12, alignItems: "center" }}>
          <div style={{ flex: 1 }}>
            <strong>Join</strong>
            <p className="hint" style={{ margin: 0 }}>
              {JOIN_HINT}
            </p>
          </div>
          {page?.consent.join ? (
            <button type="button" disabled={busy} onClick={(e) => void allowJoin(e, false)}>
              Revoke
            </button>
          ) : (
            <button type="button" disabled={busy} onClick={(e) => void allowJoin(e, true)}>
              Allow
            </button>
          )}
        </div>
        <div className="consent-row" style={{ display: "flex", gap: 12, alignItems: "center" }}>
          <div style={{ flex: 1 }}>
            <strong>Record</strong>
            <p className="hint" style={{ margin: 0 }}>
              {RECORD_HINT}
            </p>
          </div>
          {page?.consent.record ? (
            <button type="button" disabled={busy} onClick={(e) => void allowRecord(e, false)}>
              Revoke
            </button>
          ) : (
            <button type="button" disabled={busy} onClick={(e) => void allowRecord(e, true)}>
              Allow
            </button>
          )}
        </div>
      </div>

      <StatusLine note={note} />

      {!page?.live && (
        <form className="journal-add" onSubmit={(e) => void start(e)}>
          <label>
            Meeting title
            <input
              value={title}
              placeholder="Weekly standup"
              onChange={(e) => setTitle(e.target.value)}
            />
          </label>
          <button type="submit" disabled={busy || !page?.consent.join}>
            Start meeting
          </button>
        </form>
      )}

      {page?.live && (
        <div className="live-meeting" aria-label={`Live: ${page.live.title}`}>
          <h3>
            Live · {page.live.title}
          </h3>
          <form className="journal-add" onSubmit={(e) => void saveNotes(e)}>
            <label>
              Live notes (follow)
              <textarea
                value={notes}
                rows={4}
                placeholder="Decisions and action items as they land…"
                onChange={(e) => setNotes(e.target.value)}
              />
            </label>
            <button type="submit" disabled={busy || !page.consent.record}>
              Update notes
            </button>
          </form>
          <form className="journal-add" onSubmit={(e) => void sendTalk(e)}>
            <label>
              In-meeting Talk
              <input
                value={talk}
                placeholder="Ask Loadout during the meeting…"
                onChange={(e) => setTalk(e.target.value)}
              />
            </label>
            <button type="submit" disabled={busy || !talk.trim() || !page.consent.record}>
              Talk
            </button>
          </form>
          <form className="journal-add" onSubmit={(e) => void addSpeech(e)}>
            <label>
              Speaker
              <input
                value={speechWho}
                placeholder="Name"
                onChange={(e) => setSpeechWho(e.target.value)}
              />
            </label>
            <label>
              Speech line
              <input
                value={speechText}
                placeholder="What they said"
                onChange={(e) => setSpeechText(e.target.value)}
              />
            </label>
            <button type="submit" disabled={busy || !speechText.trim() || !page.consent.record}>
              Add speech
            </button>
          </form>
          <LineList lines={page.live.transcript} />
          <button type="button" disabled={busy} onClick={() => void endMeeting()}>
            End meeting
          </button>
        </div>
      )}

      <form className="journal-add" onSubmit={(e) => void search(e)}>
        <label>
          Search transcripts
          <input
            value={query}
            placeholder="Find a decision, name, or phrase"
            onChange={(e) => setQuery(e.target.value)}
          />
        </label>
        <button type="submit" disabled={busy || !query.trim()}>
          Search
        </button>
      </form>
      {page && query.trim() && page.hits.length === 0 && (
        <p className="empty">No matches.</p>
      )}
      {page && <HitList hits={page.hits} />}

      {page === null && !note && <p className="empty">Loading meetings…</p>}
      {page && !page.live && page.past.length === 0 && page.hits.length === 0 && (
        <p className="empty">{MEETINGS_EMPTY}</p>
      )}
      {page && page.past.length > 0 && (
        <div aria-label="Past meetings">
          <h3>Past</h3>
          {page.past.map((m) => (
            <article key={m.id}>
              <h4>{m.title}</h4>
              <LineList lines={m.transcript} />
            </article>
          ))}
        </div>
      )}
    </section>
  );
}

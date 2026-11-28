import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { MeetingsAct, MeetingsPage } from "./api";
import { MEETINGS_EMPTY, Meetings, REDACT_PII_HINT, REDACT_PII_TITLE } from "./Meetings";

const api = vi.hoisted(() => ({
  getMeetings: vi.fn<(query?: string) => Promise<MeetingsPage>>(),
  meetingsAct: vi.fn<(act: MeetingsAct, query?: string) => Promise<MeetingsPage>>(),
}));

vi.mock("./api", async (importOriginal) => {
  const real = await importOriginal<typeof import("./api")>();
  return { ...real, getMeetings: api.getMeetings, meetingsAct: api.meetingsAct };
});

function emptyPage(over: Partial<MeetingsPage> = {}): MeetingsPage {
  return {
    consent: { join: false, record: false, redactPii: false },
    live: null,
    past: [],
    hits: [],
    query: "",
    ...over,
  };
}

describe("Meetings", () => {
  beforeEach(() => {
    api.getMeetings.mockReset().mockResolvedValue(emptyPage());
    api.meetingsAct.mockReset();
  });

  it("loads empty and requires pointer Allow before start", async () => {
    render(<Meetings />);
    expect(await screen.findByText(MEETINGS_EMPTY)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Start meeting" })).toBeDisabled();

    api.meetingsAct.mockResolvedValueOnce(
      emptyPage({ consent: { join: true, record: false, redactPii: false } }),
    );
    await userEvent.click(screen.getAllByRole("button", { name: "Allow" })[0]!);
    await waitFor(() => expect(api.meetingsAct).toHaveBeenCalledWith({ kind: "setJoin", on: true }, ""));
    await waitFor(() => expect(screen.getByRole("button", { name: "Start meeting" })).toBeEnabled());
  });

  it("starts a meeting, follows notes, talks, ends, and searches", async () => {
    api.getMeetings.mockResolvedValue(
      emptyPage({ consent: { join: true, record: true, redactPii: false } }),
    );
    const live: MeetingsPage = emptyPage({
      consent: { join: true, record: true, redactPii: false },
      live: {
        id: "mtg1",
        title: "Standup",
        status: "live",
        startedMs: 1,
        endedMs: null,
        liveNotes: "",
        transcript: [],
      },
    });
    api.meetingsAct
      .mockResolvedValueOnce(live)
      .mockResolvedValueOnce({
        ...live,
        live: {
          ...live.live!,
          liveNotes: "Ship Friday",
          transcript: [
            { id: "l1", atMs: 2, kind: "note", speaker: "", text: "Ship Friday" },
          ],
        },
      })
      .mockResolvedValueOnce({
        ...live,
        live: {
          ...live.live!,
          liveNotes: "Ship Friday",
          transcript: [
            { id: "l1", atMs: 2, kind: "note", speaker: "", text: "Ship Friday" },
            { id: "l2", atMs: 3, kind: "talk", speaker: "", text: "Any blockers?" },
          ],
        },
      })
      .mockResolvedValueOnce(
        emptyPage({
          consent: { join: true, record: true, redactPii: false },
          past: [
            {
              id: "mtg1",
              title: "Standup",
              status: "ended",
              startedMs: 1,
              endedMs: 4,
              liveNotes: "Ship Friday",
              transcript: [
                { id: "l1", atMs: 2, kind: "note", speaker: "", text: "Ship Friday" },
                { id: "l2", atMs: 3, kind: "talk", speaker: "", text: "Any blockers?" },
              ],
            },
          ],
        }),
      )
      .mockResolvedValueOnce(
        emptyPage({
          consent: { join: true, record: true, redactPii: false },
          query: "blockers",
          hits: [
            {
              meetingId: "mtg1",
              meetingTitle: "Standup",
              lineId: "l2",
              kind: "talk",
              speaker: "",
              text: "Any blockers?",
              atMs: 3,
            },
          ],
        }),
      );

    render(<Meetings />);
    await screen.findByRole("button", { name: "Start meeting" });
    await userEvent.type(screen.getByLabelText("Meeting title"), "Standup");
    await userEvent.click(screen.getByRole("button", { name: "Start meeting" }));
    await waitFor(() =>
      expect(api.meetingsAct).toHaveBeenCalledWith({ kind: "start", title: "Standup" }, ""),
    );
    expect(await screen.findByText("Live · Standup")).toBeInTheDocument();

    await userEvent.type(screen.getByLabelText("Live notes (follow)"), "Ship Friday");
    await userEvent.click(screen.getByRole("button", { name: "Update notes" }));
    await waitFor(() =>
      expect(api.meetingsAct).toHaveBeenCalledWith(
        { kind: "followNotes", notes: "Ship Friday" },
        "",
      ),
    );

    await userEvent.type(screen.getByLabelText("In-meeting Talk"), "Any blockers?");
    await userEvent.click(screen.getByRole("button", { name: "Talk" }));
    await waitFor(() =>
      expect(api.meetingsAct).toHaveBeenCalledWith({ kind: "talk", text: "Any blockers?" }, ""),
    );

    await userEvent.click(screen.getByRole("button", { name: "End meeting" }));
    await waitFor(() => expect(api.meetingsAct).toHaveBeenCalledWith({ kind: "end" }, ""));

    await userEvent.type(screen.getByLabelText("Search transcripts"), "blockers");
    await userEvent.click(screen.getByRole("button", { name: "Search" }));
    await waitFor(() =>
      expect(api.meetingsAct).toHaveBeenCalledWith({ kind: "search", query: "blockers" }, "blockers"),
    );
    expect(await screen.findByText(/Any blockers\?/)).toBeInTheDocument();
  });

  it("lists extracted action items on a past meeting and a clean empty fallback", async () => {
    api.getMeetings.mockResolvedValue(
      emptyPage({
        past: [
          {
            id: "mtg1",
            title: "Standup",
            status: "ended",
            startedMs: 1,
            endedMs: 4,
            liveNotes: "",
            transcript: [],
            actionItems: [
              {
                owner: "user",
                action: "file the bug",
                due: "Friday",
                sourceQuote: "user will file the bug by Friday",
              },
            ],
          },
          {
            id: "mtg2",
            title: "Chat",
            status: "ended",
            startedMs: 5,
            endedMs: 6,
            liveNotes: "",
            transcript: [{ id: "l1", atMs: 5, kind: "speech", speaker: "host", text: "No blockers" }],
            actionItems: [],
          },
        ],
      }),
    );
    render(<Meetings />);
    expect(await screen.findByText(/file the bug/)).toBeInTheDocument();
    expect(screen.getByText(/due Friday/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Copy action items" })).toBeInTheDocument();
    expect(screen.getByText("No action items.")).toBeInTheDocument();
  });

  it("sends setRedactPii when the transcript toggle is flipped", async () => {
    api.getMeetings.mockResolvedValue(emptyPage());
    api.meetingsAct.mockResolvedValueOnce(
      emptyPage({ consent: { join: false, record: false, redactPii: true } }),
    );
    render(<Meetings />);
    const sw = await screen.findByRole("switch", { name: REDACT_PII_TITLE });
    expect(sw).toHaveAttribute("aria-checked", "false");
    expect(screen.getByText(REDACT_PII_HINT)).toBeVisible();
    await userEvent.click(sw);
    await waitFor(() =>
      expect(api.meetingsAct).toHaveBeenCalledWith({ kind: "setRedactPii", on: true }, ""),
    );
    expect(await screen.findByRole("switch", { name: REDACT_PII_TITLE })).toHaveAttribute(
      "aria-checked",
      "true",
    );
  });
});

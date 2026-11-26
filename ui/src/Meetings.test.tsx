import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { MeetingsAct, MeetingsPage } from "./api";
import { MEETINGS_EMPTY, Meetings } from "./Meetings";

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
    consent: { join: false, record: false },
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

    api.meetingsAct.mockResolvedValueOnce(emptyPage({ consent: { join: true, record: false } }));
    await userEvent.click(screen.getAllByRole("button", { name: "Allow" })[0]!);
    await waitFor(() => expect(api.meetingsAct).toHaveBeenCalledWith({ kind: "setJoin", on: true }, ""));
    await waitFor(() => expect(screen.getByRole("button", { name: "Start meeting" })).toBeEnabled());
  });

  it("starts a meeting, follows notes, talks, ends, and searches", async () => {
    api.getMeetings.mockResolvedValue(
      emptyPage({ consent: { join: true, record: true } }),
    );
    const live: MeetingsPage = emptyPage({
      consent: { join: true, record: true },
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
          consent: { join: true, record: true },
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
          consent: { join: true, record: true },
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
});

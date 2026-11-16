# Chat semantic zoom: design note

Zooming out of a long chat folds its messages into topic groups, so it's quick
to get back to an earlier part of the session. This note covers the grouping
approach, what it costs, and what ships in which step. It applies to Main and
to regular chat tabs, which share the same chat view and the same session
files (`{config}/sessions/shell-*.jsonl`).

## Zoom levels

| Level | Shows | Rows for a 1,000-message chat |
| --- | --- | --- |
| Messages | The chat as it is today | 1,000 |
| Topics | One card per topic group: title, one-line summary, time range, message count | about 60–100 |
| Days | One row per day (or per session, for turns saved before timestamps existed), with its topic titles | a handful |

Controls:

- Ctrl+scroll and trackpad pinch on the chat log step one level. Both arrive
  as `wheel` events with `ctrlKey` set, so a non-passive listener on the log
  calls `preventDefault()` to stop the webview zooming the whole window (the
  text-size setting stays in charge of that).
- Ctrl+Shift+O steps out, Esc steps back in. Ctrl+Shift+O is free in the
  chat's shortcut set (`ui/src/chatKeys.ts`, `shellKeys()`), and it's added
  to the shortcut sheet.
- A small three-step control beside the chat tabs shows the current level.
- Clicking a group zooms in and scrolls to its first message. Zooming out and
  back in without clicking returns to the message that was at the top.

## Grouping

The unit is a **turn**: one user message and the replies and tool rows after
it. The image a tool returns comes back as a user message; it stays in its
turn and isn't counted as an ask or a message. A compaction's summary, ledger
and standing rules are in no turn. Groups are runs of turns, cut at a
boundary.

1. **Time gap.** A new group starts when 30 minutes or more pass between two
   turns. Session files have no per-message times today, so each turn's
   closing `usage` line gains an `at` field (Unix ms): the time the turn was
   saved, at its end, so one run of 30 minutes or more also starts a group.
   Readers already ignore unknown fields, and the session rewrite keeps it.
   A compaction's own usage line carries one too. A compaction keeps some
   of the messages it replaced (the last ask, maybe its reply): each one is
   matched, newest first and in order, to a replaced message that shows the
   same (an image the model's copy lost to the image budget still counts as
   that image) and keeps that one's time; the rest (its summary and rules)
   take the compaction's time. A turn that compacted as it ran says on its
   line how many of the kept items at the end are its own (`"own": 3`): the
   run tracks where its items went through each compaction, by matching them
   newest first. Those were never saved on their own lines, so they take the
   compaction's time and an identical older ask such as "continue" never lends
   it one. Deleting one of them lowers the count. Older turns have no time; their groups show the
   session's start and last-saved times as the range instead.
2. **Topic shift.** Each turn's user text is embedded with the local static
   model the app already ships for memory recall (`potion-base-8M`, 256
   dimensions, CPU only, no network once downloaded). A new group starts when
   a turn's cosine similarity to the running centroid of the open group drops
   below a threshold (starting at 0.35, tuned on real chats), once the group
   has at least 3 turns. A group also closes at 40 turns. When the model
   isn't downloaded (or memory embeddings are off), the same rule runs on a
   hashed bag of stemmed words (1,024 buckets), with its own threshold (0.08).
   Words are stemmed roughly ("files", "filed" and "file" match), and
   negative contractions ("don't") are skipped. Indic and Southeast Asian
   combining signs stay inside their words; those scripts' punctuation ends
   one. Scripts written without spaces (Thai, Lao, Myanmar, Khmer, kana and
   CJK ideographs), and Hangul, whose words carry their particles, count
   each pair of characters as a word; a Latin word inside them stays a word
   of its own.
3. **Labels.** The title comes from the group's own words, with the existing
   unused topic helpers (`topics_from_turns`, `short_auto_title`). The one-line
   summary is the group's first user line, clipped to about 90 characters. No
   model call, so it works offline, costs nothing, and scratch chats never
   leave the device.

Groups cover the messages the chat view can show. A chat that was compacted
gets one "Earlier, compacted" row on top with its message count; it isn't
clickable, because the chat view can't show those messages either.

## Cache

- `{config}/zoom/<chat-id>.json` holds, per turn, a hash of its role and text
  and its vector (int8, base64, about 344 characters for 256 dimensions).
  Times and groups come from the session file on each open; grouping the
  vectors is quick. A fresh vector is grouped from its stored int8 form, and
  a reused one is stored again as it was read, so a reopen groups exactly as
  the first open did. Scratch chats are grouped in memory only
  and never written.
- On open, the cache is checked against the session: the longest prefix of
  matching turn hashes is kept, and only the turns after it are embedded
  again. A new message reopens at most the last group. Delete message,
  compaction and Main's Clear change the hashes and rebuild from the first
  turn that differs.
- Deleting a chat deletes its zoom file, and a zoom that finishes after the
  delete removes the file it wrote.
- A compacted chat's "Earlier, compacted" count is every message a
  compaction took out of view: what each one replaced, less the messages it
  kept. A kept message deleted later still counts, since its saved copy is
  under the compaction.

## Cost per 100 messages

| Part | Cost |
| --- | --- |
| Embedding (local static model) | $0 in tokens; one-time 30.5 MB model download the app already does for memory recall |
| Storage | about 38 KB for 100 turns (int8 vectors in base64 JSON) |
| Labels (local words) | $0 |
| Optional later: model-written titles | about $0.015–0.02 on grok-4.3-class pricing, $0 on Local AI; labeled once when a group closes, under the background budget share |

The token estimate assumes about 8 groups per 100 messages, 100 tokens per
message and 60 output tokens per group, priced from
`crates/grokhub-core/data/model_profiles.toml`. Model titles are not part of
this feature; they would come later behind a setting.

## Scroll and size

- Today "Show earlier" reloads the transcript and jumps to the bottom. Jumping
  to a group above the loaded part loads enough lines to include it, then
  scrolls to its first row without going through the follow-the-end effect.
- The message list isn't virtualized. Rows get `content-visibility: auto`
  with an intrinsic size, so a 1,000-row log lays out only what's on screen.
  The topic view is small enough to render whole.
- Moving between levels is a short fade and scale, off under reduced motion.

## Look

Black background, red `#D7263D` for group outlines and headers, and no gray
fills. Amber stays on the chat's one main action (Send). Muted text uses the
theme's text tokens.

## Steps

1. **Backend** (this PR): the `at` field, the grouping engine and its cache,
   and a `chat_zoom` command, with tests for the boundaries, the incremental
   cache and restart.
2. **UI**: the levels, controls, jump-back and scroll hold, with tests for
   the jump-back and the controls.

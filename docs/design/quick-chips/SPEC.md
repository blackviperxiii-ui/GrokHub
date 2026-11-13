# Loadout quick chips (spec v3)

Nov 5, 2026. Replaces `docs/superpowers/specs/2026-08-14-quick-chips-design.md`.

Quick chips are the few next steps Loadout offers under the chat box, on Home and in the empty command palette. One engine in `grokhub-core` feeds every surface in both apps: the egui cabin and the desktop and web shell. No setting tunes it, no model is picked for it, and it never makes a model call of its own.

## What a person sees

- At most three chips in a row, in one look everywhere: 48 px tall, 16 px text, 8 px apart, one primary chip.
- Each chip has a ×, a tip that says why it is there, and Alt+1 to Alt+3. Left and Right move along the row.
- A tap follows the chip's kind:
  - **Ask**: the words go to the chat. With text in the box, they are added to it, never put in its place.
  - **Go**: opens a page.
  - **Do**: an app action that can be undone. The cabin shows Undo for 30 seconds.
  - **Review**: a command. It opens a review card and never runs on the tap.
- × means "not now". The chip rests, and the ranker learns from it.
- Chips hide while a reply streams. A reply's own follow-ups join the row when it ends, and keep the mode the reply was written in.
- Settings → Behavior → Suggestions turns every row off. It is on by default.

## Pipeline

1. **Pool** (`build_quick_chips`): every source adds candidates with an engine score.
   - The chat and draft give stage chips and templates.
   - Habits come in two forms: frecency, and next-step odds from the last chip.
   - Context signals include the time, weekday, quiet hours, low disk, a waiting update, a game, a protected anti-cheat and a file in the tray.
   - Each feature's offers come in through `ChipInput.extra`: code resume, focus help, model drop, backup, tidy, radar, game and skill.
   - The reply's own chips come in through `reply_chips` (see below).
2. **Vetoes and filters**:
   - While a game is up, nudges hold back.
   - While a protected anti-cheat runs, chips that touch the game are hidden.
   - Copy checks, dismissed chips and chips that rest after five unpicked showings in three days are dropped.
3. **Lane lean** (cabin): the coding or life lane nudges near-tie scores before ranking. The lead is never more than two thirds of a tenth of the top score.
4. **Rank** (`rank_quick_chips`):
   - Thompson sampling over a diagonal Bayesian logistic model of 12 features.
   - What the person did with each chip before (picked, passed over, hidden) is read from the learning engine's belief about that chip (`learn`, subject `chip:<key>`), so the row and the "What I've learned" page use the same numbers. Forget clears it, "Always" reads as a sure yes, and a chip marked wrong is never shown.
   - MMR keeps one chip per intent.
   - A gate hides rows that are unlikely to be picked once 50 rows have been seen.
   - The draw is seeded per boundary (a turn, a page, the hour), so a row holds still while the person types.
   - Each shown chip logs its propensity.
5. **Holdout**: one boundary in twenty shows no row and is logged, so the log can tell whether chips speed up the next step. The first-run guide is never held back.
6. **Log and learn** (`chip_store`):
   - `chips.json` v3, behind a lock, with up to 400 rows.
   - Each row records what it showed, each chip's slot, propensity and features, the outcome, and when the next message came.
   - Picks and hides teach the ranker. A row replaced without a pick teaches it a little.

## The reply's own chips

The chat prompt's cached rules ask the model to end a reply with up to three lines in a hidden `<chips>` block. The rule is under 700 bytes.

- **Lines:** each line is `kind|target|label`. `ask` lines become chat chips, and `open` lines must name a real page.
- **Filtering:** a stream filter keeps the block out of the chat, the saved history and speech.
- **Checks:** lines that send, delete, pay or buy are dropped, as are lines that fail the copy checks.
- **Auto-off:** if reply chips are picked at under half the rate of local chips after 40 showings, the rule leaves the prompt for 14 days.
- **What's not used:** a btw reply, a cancelled turn and a halted turn give no follow-ups. The rule stays in the prompt on those turns so the cache holds.

## Shell boundary

The shell gets wire chips over IPC (`quick_chips`, `chip_outcome`). The checks:

- A wire chip is only Ask or Go. A Go chip must map to a page the shell has.
- Mode values, Undo, Do and Review never cross.
- An outcome counts only for a chip from the last row shown on that surface, so a page can't teach the ranker a chip it made up.
- Every read, change and save of chip memory happens under one lock.

## Proving it (`chip_replay_tests`, `chip_health`)

- **Replay:** a scripted month of steady habits, scored over its last three weeks.
  - Recall@3 is 0.90 for the engine against 0.36 for frecency on the same rows.
  - MRR@3 is 0.84 against 0.21.
  - The test requires a lead of at least 10 points.
- **Speed:** ranking 200 candidates against a full log takes a p99 of 0.74 ms in an optimized build. The test requires under 1 ms optimized and under 40 ms in a test build. Chip memory stays far under 5 MB, at about 120 KB full.
- **Health:** Settings → Labs → Diagnostics shows these numbers, read from the log in memory:
  - rows shown and held back
  - pick and hide rates
  - the middle time to the next step with chips and without
  - the best-picked sources

## Not built

- Calibration error, blind dismiss and undo-after-chip numbers.
- Signals that have no source in the app yet: thumbs-down, blocked workboard cards, automation failures, a finished background job, and offline or usage cap.
- Slash skills ranked by habit in the shell's slash menu.
- A local-model fallback for surfaces with nothing to show.

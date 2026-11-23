---
name: technical-writing
description: Layered writing standard (Diátaxis, Google developer style, STE, Global English) for docs, RFCs, readmes, PR descriptions, commit messages. /technical-writing.
---

# Technical writing

Write so a tired engineer gets it on first read. Above all layers:

- Cut every word that does no work. "In order to" → "to".
- Short everyday words: "use", not "utilize".
- If a rule makes a sentence worse, fix it another way or leave it.
- The codebase is the word list: real symbol, file, flag, command names. No invented jargon; define a named pattern on first use.

**Rhythm.** Mix sentence lengths. Split a sentence with two thoughts; keep a long one with one. Have a view where the mode allows. Specific beats sterile: "a column rename fails the build".

## 1. Pick the mode (Diátaxis)

One doc, one mode. Action+learning **tutorial**: say what they'll build, visible result every step, show expected output, "we" + commands. Action+work **how-to**: a real problem, assume competence, action only, forks allowed, title by task. Understanding+work **reference**: describe only, dry, complete, mirror the code's structure, generate from code when possible. Understanding+learning **explanation**: one bounded topic, the why, trade-offs; opinion only here. Don't mix; split and link.

## 2. Sentences (Google style)

"You", present tense, active voice. Instructions as commands. Condition before instruction. Common case first. No "simply", "easy", "please", no pre-announcing. Link text names the destination, never "click here". Headings carry the point, sentence case; tasks are verb phrases. Numbered lists for sequences, bullets otherwise, introduced by a full sentence, items parallel. Code in code font, UI in bold, serial commas, no "etc.".

## 3. One statement at a time (STE)

One instruction per sentence. Split instructions over ~20 words, others over ~25. Warning before the step. Keep "the" and "a". One meaning per word, one word per action. Avoid "-ing" where you can.

## 4. One reading only (Global English)

Keep "only"/"not" next to what they modify. Break noun stacks. Every "it"/"this" points at one thing; never "this" for a whole clause. Don't drop verbs. Keep "that". Periods, not semicolons or em dashes. No slashes, no "(s)", no idioms or Latin abbreviations. One name per thing everywhere.

## Repo specifics

Run **unslop** on everything. PR bodies and commits: all layers but Diátaxis; a reviewer reads the PR body in under a minute; link logs and tables, don't paste. UI strings follow product copy rules. Real paths; every count true at the commit, with the command that regenerates it.

**Before:** "Configuration of the budget script parameters is performed via budget.json. Note that running with --write should only be done when lowering it." **After:** "`budget.mjs` reads the budget from `budget.json`. Run `budget.mjs --write` only to lower it."

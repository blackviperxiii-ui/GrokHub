# Prompts worth copying

Swap in the real paths, skills, and done checks. Informal wording works.

## Understand

- `/viper-mode read <thread>. restate the underlying issue in your own words, in plain english.`
- `/in-your-own-words <long or messy ask>`
- `/sharpen <the prompt you're about to send>`
- `/viper-mode investigate why <symptom>. give me what we know, what data you used, and your best hypotheses. don't change any code yet.`
- `use /how to understand <subsystem>. then use /why to find out why it broke recently.`
- `/recall my work on <topic> from last week, then read <issue>.`
- `/teach me why you implemented it this way and not <other way>. what did you trade off?`
- `/viper-mode take over this branch. read the decision log, find what's done, and continue. don't redo finished work.`

## Build

- Bug: `/viper-mode <symptom>. repro first, then fix and verify.`
- Bug in an app: `/viper-mode repro this with /verify-<app>. if it repros on main, fix it and show me a video as proof.`
- Bug with a cheap test: `/viper-mode repro <bug> first. if there's a cheap test path, /tdd it. then fix and rerun.`
- Feature: `/viper-mode add <behavior>. <current output> stays byte-identical. verify both.`
- Refactor: `/viper-mode move <code> into one module, zero behavior change. record the current output first and prove it's unchanged after.`
- Perf: `/viper-mode <operation> takes <time> on <fixture>. trace it, fix the measured cause, show me before and after.`

## Design and plan

- `/viper-mode prototype a few options for <feature>. take screenshots or videos for me to compare.`
- `/viper-mode we need <feature>. /architect it first, and answer open questions with prototypes. let me review before proceeding.`
- `/viper-mode write a tutorial for how i would use <new package> first. then /teach me why it beats the current one.`
- `ask /arena for a second opinion on this thread and our approach.`
- `/viper-mode turn this design into a plan. small verifiable PRs, each with its own verification steps.`
- `/viper-mode plan the migration of <library> to <target>. small verifiable PRs. the result must match the original exactly, bugs included.`

## Review and ship

- `/interrogate the whole branch, but skeptically. don't change anything yet. no nitpicks unless it's a real bug or regression.` Read the Dropped list too.
- `/create-formal-verification-skill prove <property> holds in <module>, even when <steps> interleave or retry.` After merges or refactors in that code: `/maintain-formal-verification-skill`.
- `/swarm check every package under <dir> against its check script. one worker per package. one report.`
- `/viper-mode open the pr. small ordered commits, evidence in the description.`
- `/viper-mode babysit this pr. get it green.` For status only: `/viper-mode check on pr <number>. anything outstanding?`
- `/viper-mode land the stack.`
- `/qa-walkthrough walk the app's beta end to end before we promote to main.` Read the verdict and its blockers first.
- `/av-qa-walkthrough verify the player on the reference clips before we ship.`
- `/bag-and-tag promote beta to main and cut the release.` It runs only when you ask for it in so many words.

## Feel, craft, and upkeep

- `/land-it the app works but doesn't feel right. dive the whole app and rank the low-effort fixes.`
- `/elevate make <screen> feel premium. reuse our tokens. show me before and after in the real UI.`
- `/optimize <screen or operation> is slow. measure the real build, fix the root cause, show me before and after.`
- `/stageshow build a fake <product> dashboard with an animated cursor demo for the landing page hero.`
- `/updater check what in this app needs updating and bump what's safe.`
- `/database-architect design the tables for <feature>, with a migration that doesn't lock live tables. test it on a scratch database.`
- `/modern-web-guidance build <ui element> the modern platform way, with the fallbacks the guide names.`
- `/chrome-extensions add a side panel to this extension and get it ready for Web Store review.`

## Away and back

- `/viper-mode im going to bed. <goal> in a fresh worktree off <base>. done means <checks>. keep a decision log. don't ask me before committing. /loop until done. if you're truly stuck after a few hours, stop and write up why.`
- On a host without `/loop`, drop that phrase and write `keep going until every check passes`.
- `/follow-through keep a ledger for these tasks. check in at each milestone and tell me when one stalls.`
- `/show-me-your-work catch me up on what you did last night.` Read its Attention section first.
- `/ketchup` catches you up on this chat since your last message and lists every call that waits on you.
- `/viper-mode full autopilot on this queue. each item is independent.`
- `/viper-mode autopilot these changes, but don't merge. i'll land them.`
- `/reflect capture what we learned so the next run doesn't repeat it.` Approve only edits that change a future decision.
- `/bro` restates the last reply in plain words.

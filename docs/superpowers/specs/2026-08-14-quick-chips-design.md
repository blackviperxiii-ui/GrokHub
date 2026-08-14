# Predictive quick chips

**Date:** 2026-08-14  
**Version:** 2.0.0 (do not bump)  
**Override:** Native Rust only. Steal the Electron chip *mechanism*, not grok.com.

## Spec

Chips sit **above** the composer pill and change as the user types, as the thread moves, and as habits accumulate.

- Local rank is instant: stage (empty / mid / error / tools / long), last assistant, draft prefix, hour affinity, click/dismiss memory.
- Fast mode (`grok-3-mini-fast`) suggests 3–4 chips when the context fingerprint changes. Debounce 1.2s. Never block chat.
- Visible cap 5, hard cap 8. Mix kinds (chat / shell / nav / mode). One mode chip max.
- Click sends or navigates. × dismisses and soft-avoids. Typed prompts reinforce matching habits.
- Secrets never persist in `chips.json`. `is_plain_text` gates every stored value.
- Composer is a dark pill: placeholder **Ask anything**, plus, Auto/mode, mic, white send.

Do not clone grok.com’s game tree or website sidebar. Keep the Electron-look rail and titlebar.

Automations and Skills steal the Grok catalog chrome: large title, white pill action, rounded cards, Suggested / Personal grids. Skills and Connectors share one page with tabs. Suggested automations must parse as real night jobs.

Catalog honesty: no Outlook / Gmail / Drive / Office / stock / video generation. Suggested skills are cabin verbs (HOST_CMD, workboard, Imagine images, verify). The only live connector card is GitHub (Who am I / List repos). Website hosts stay an allowlist, not fake apps. Imagine copy is images only. No SuperGrok quota chip.

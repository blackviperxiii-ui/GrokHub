---
name: stageshow
description: Build faux product UI that looks like a real product, as a still or an animated cursor demo. Browser and app windows, dashboards, phones, notifications, terminals, bento grids and walkthroughs, framed for a marketing page, feature card or docs hero. Use for any fake or mocked UI, product demo animation, hero or feature-card illustration, or animated cursor in an interface. React and plain CSS. /stageshow.
---

# Stageshow

Faux product UI stills, cursor demos and bento grids, built in React.

**Before planning, read `GUIDE.md` in this folder in full.** It is the whole skill: workflow, gold examples, rules. Every path it names (`references/`, `examples/`, `kit/`, `scripts/`, `teaching/`) is relative to this folder.

Short form:

1. Plan first: medium and size, look, a 3 to 5 beat story with a payoff.
2. Copy the closest gold example from `examples/`; never build from scratch.
3. Build with `kit/`; mark cursor targets with `data-target`, measure with `rect()`.
4. Film and fix: `npm install` here once, then `node scripts/film.mjs ... --gate` (needs Node 20.19+ and local Chrome). A passing gate is necessary, not sufficient.

Credit: Stageshow by Benji Taylor, MIT (`LICENSE`, `NOTICE.md`). Vendored source and local changes: `VENDORED.md`.

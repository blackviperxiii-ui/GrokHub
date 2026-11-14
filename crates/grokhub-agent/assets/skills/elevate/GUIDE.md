# Elevate — detail

Use after the short playbook in `SKILL.md`. Goal: raise visual and interaction craft with evidence, not a new diagnosis report.

## Not these skills

| Skill | Job |
|-------|-----|
| **elevate** | Improve UI craft (hierarchy, polish, motion, states, modernization) |
| **land-it** | Diagnose whole-app feel misses → ranked fix report |
| **qa-walkthrough** | Ship gate: bugs, security, tests |
| **stageshow** | Faux product stills / demos |
| **optimize** | Runtime/perf cost, not aesthetics |

## Premium vs cheap (quick tells)

**Premium:** work area dominates; nav/support chrome quieter; structure felt via soft separators and space; icons only when recognition needs them; layered shadow + crisp translucent border when a surface earns it; nested radii concentric; empty state names the gap and offers one clear next step; focus is border/content/state (not glow-only).

**Cheap:** equal visual weight everywhere; divider/border pile-up; icon ornament and colored icon plates; nested cards; muddy flat shadows or neon glass; empty decorative blob; happy-path-only screens.

Linear craft cues: "don't compete for attention you haven't earned"; "structure should be felt not seen."

## Hierarchy pass

1. Type scale + weight first; color second; surfaces last.
2. Spacing from named tokens; within-group gaps smaller than section breaks.
3. Semantic color roles; check light, dark, and increased contrast (text ≈4.5:1; non-text state ≥3:1).
4. At 200% text size, hierarchy and reflow still work.

## Motion budget

| Kind | Duration |
|------|----------|
| Micro (hover/focus) | 100–200ms |
| Standard show/hide | 200–300ms |
| Page / layout | 300–400ms |

- Purpose: feedback, continuity, orientation — never decoration.
- Enter ease-out; exit ease-in; move ease-in-out.
- Prefer `transform` + `opacity`. Shared-element / View Transitions when the same object changes place.
- Parent before child; sibling stagger ~30–80ms; co-located properties share one timing.
- `prefers-reduced-motion: reduce` → usable parallel (opacity/instant), not meaning-only-in-motion and not a broken snap.

## State matrix (per control / surface)

Idle, hover (pointer:fine), focus-visible, pressed, pending, success, error, disabled. Plus empty and loading for collections. Keyboard path required.

## Modernization without trend-chasing

**Dated:** chrome accumulation; equal-weight toolbars; heavy borders; theme as palette swap only; cool oversaturated greys without roles.

**Current:** calmer hierarchy; tokenized type/space/color; softer structure; density with a clear primary work area.

Copy hierarchy discipline from strong products; do not skin-copy their brand look.

## AI-default gates (fail → redesign)

Reject when present without brand intent:

- Default Inter / Roboto / Arial as the expressive hero stack
- Purple→indigo or purple→cyan decorative gradients
- Warm-cream + terracotta serif cliché; broadsheet hairline zero-radius "premium"
- Rows of equal-weight feature cards; cards nested in cards
- Glass + neon glow; bounce/elastic on every hover; fade-up on every scroll
- Invented palette when product tokens exist; missing empty/error/focus/recovery

## Code craft

- Tokens for semantic color, type, space, surface; components consume tokens.
- Set `color-scheme` on root and theme-color meta so browser chrome matches.
- Semantic HTML / native controls; visible focus; density adapts by breakpoint.
- One job per section: one headline, short support, no competing promo chrome.

## Stop / handoff

- Stop when the named surface meets criteria (**Laziness Protocol**).
- Whole-app "doesn't hit right" diagnosis → **land-it**.
- Ship confidence → **qa-walkthrough**.

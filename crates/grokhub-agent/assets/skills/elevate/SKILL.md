---
name: elevate
description: Raise UI craft toward premium, modern, fluid feel. Hierarchy, type, color, space, motion, states, anti-slop. Not a feel diagnosis. /elevate
---

# Elevate

Improve an app's UI toward spectacular craft. Not **land-it** (diagnose misses) or **stageshow** (faux demos).

**Read `GUIDE.md` in this folder** for checklists (premium tells, motion budget, AI gates, states).

Cite **viper-mode**, **principle-experience-first**, **principle-subtract-before-you-add**, **principle-minimize-reader-load**. Prose: **sharpen**/**unslop**.

## Playbook

1. **Restate.** Name the surface and the feel it should have (**in-your-own-words**). Look at it in the real UI first and screenshot the before. Reuse the product's brand tokens. Never invent a second design system beside the one that exists.
2. **Subtract first.** Remove what competes for attention before adding polish: stacked borders, cards of equal weight, ornamental icons, nested surfaces.
3. **Set hierarchy in order.** Type, then space, then color, then surface. The work area leads and the chrome recedes. One primary action per view.
4. **Cover every state.** Empty, loading, error, success and focus, in light and dark, and at 200% text size. A screen that is only designed for the happy path is not finished.
5. **Add motion with a purpose.** Feedback, continuity or orientation only, inside the timing budget in the guide. Give `prefers-reduced-motion` a usable parallel path.
6. **Gate the AI defaults.** Check the result against the reject list in the guide and redesign anything that trips it. Web code follows **modern-web-guidance**.
7. **Prove it in the real UI.** Screenshot the after beside the before, in both themes and at narrow and wide widths, and use it with the keyboard. Diagnosis of what feels wrong goes to **land-it**. The ship gate is **qa-walkthrough**. Stop when the named surface meets its criteria.

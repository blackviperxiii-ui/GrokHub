---
name: modern-web-guidance
description: Google Chrome's modern web platform guides (HTML, CSS, client JS, a11y, forms, performance, security), read locally. Use when writing or changing web frontend UI code; not backend, infra, or tooling. /modern-web-guidance
---

# Modern Web Guidance

Google Chrome's guides, vendored at a pinned release (`VENDORED.json`). Apache-2.0 (`LICENSE`, `NOTICE.md`).

**Offline only.** Never run `npx`/`pnpx modern-web-guidance` (no `@latest`, no `install`, no `legacy-scan`): it fetches unpinned code and sends telemetry. If something insists, set `DISABLE_TELEMETRY=1` and pin `1.0.0`.

## Lookup

1. Search: `grep -i "<2-5 words>" INDEX.md` (ids, files, titles), or `grep -ril` under `guides/`. Name the UI primitive (`dialog`, `popover`, `date input`), not the framework.
2. Read the matching `guides/<area>/<id>.md` in full, plus the area's overview guide (`css`, `html`, `forms`, `accessibility`, `performance`, `security`) when it applies.
3. A guide's `retrieve "<id>"` means read `guides/*/<id>.md`.
4. Apply it; check the code against the guide and its fallbacks. Baseline Widely available needs no fallback unless the project sets a browser policy.
5. When a guide shaped the change, name its id and the rule in your summary.

`UPSTREAM_SKILL.md` is Google's original text (framework tips, browser support); its `npx` steps are replaced by this lookup. Resync: `docs/modern-web-guidance.md`.

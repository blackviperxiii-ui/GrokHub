# Notice

This folder contains the `modern-web-guidance` skill from Modern Web Guidance by Google LLC
(Chrome team), https://github.com/GoogleChrome/modern-web-guidance, licensed
under the Apache License, Version 2.0 (`LICENSE` in this folder). The pinned
release, commit and a SHA-256 of every vendored file are in `VENDORED.json`.

## Changes made by vstack

Google's files are copied unmodified, apart from these:

- Renamed: upstream `SKILL.md` → `UPSTREAM_SKILL.md`, so agent hosts load the
  vstack wrapper instead of the upstream trigger. Its text is unchanged.
- Added: `SKILL.md` (vstack wrapper with its own trigger and an offline,
  no-`npx` workflow), this `NOTICE.md`, and the generated `VENDORED.json`.
- Generated: `INDEX.md` (guide id, path and first heading) so agents can
  search locally instead of running the upstream CLI.

The rest of vstack is MIT licensed; that license does not apply to Google's
files in this folder.

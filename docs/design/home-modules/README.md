# Home Mission Control — module design refs

Authoritative spec for Order -38 (Home = Mission Control, modular tiles).

- [`SPEC.md`](SPEC.md) — catalog (36), slot grid v2, Edit Home dialog, `home_layout.json` v2, dev plan
- [`LAYOUTS.md`](LAYOUTS.md) — default / edit / performance / streamer layout notes (PNG mockups may be absent on some VMs)
- `gallery-1.png` … `gallery-5.png` — every module at L / M / S by category

Code: `grokhub_core::home` (registry, layout engine, schema) and `crates/grokhub-app/src/app/home/` (`home_layout.json` load/save).

Do not treat a `design/home-modules` git branch as mergeable product work — these files are reference only until the feature PRs land on `beta`.

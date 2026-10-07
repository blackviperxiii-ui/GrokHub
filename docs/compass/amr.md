# Compass: AMR (agent memory repo, M0)

## Owns
- The `amr/` store under the cabin config dir: `amr/README.md` (`amr_schema: 1`), `nodes/<id>.md`, `nodes/<id>.sealed` (personal or sensitive, Spike-4b), `edges/edges.jsonl`, `dreams/` (unused in M0).
- Node schema (six types, five edge relations), `NodeId` validation, and the `MemoryEngine` seam (`recall`, `remember`, `link`). `LegacyMemory` adapts SOUL/USER/MEMORY.
## Quick commands
- `cargo test -p grokhub-core amr` (schema, store, adapter tests)
- `cargo test -p grokhub-app recall_` (legacy vs amr `/recall`, set `GROKHUB_CONFIG` to a temp dir first)
## Key files
- `crates/grokhub-core/src/amr/mod.rs`: `AmrError`, `MemoryBackend`, `MemoryEngine`, `LegacyMemory`, tests.
- `crates/grokhub-core/src/amr/schema.rs`: `AMR_SCHEMA`, `NodeType`, `NodeId`, `Node::to_markdown` / `from_markdown`, `EdgeRel`.
- `crates/grokhub-core/src/amr/store.rs`: `AmrStore` (`at`, `with_sealer`, `init`, `recall`, `recall_report`, `remember`, `link`, `edges`).
- `crates/grokhub-app/src/app/slash.rs`: `recall_amr_lines` and the `/recall` branch on `memory_backend`.
## Change recipe
- Read `docs/superpowers/specs/2026-10-07-amr-m0.md` first; it lists what M0 must not do.
- New node field: extend `Node`/`NodeDraft`, keep `to_markdown(from_markdown(s)) == s`, and update `schema_round_trip_matches_the_literal_markdown_and_edge_line`.
- New write path: run text through `redact_secrets` like `AmrStore::remember` does, and honor scratch.
## What breaks it
- Writing `amr/` on the legacy path: legacy must never create it (`recall_legacy_finds_memory_line_and_skips_amr`).
- Serializing `memory_backend` when it is legacy: a default `app.json` must not grow the key (`memory_backend_defaults_legacy_and_parses_amr`).
- Ids outside `[a-z0-9-]`, 1..=96 bytes, or starting with `-`; confidence outside 0.0–1.0.
- A personal or sensitive node written as `.md`: with no sealer or no key, `remember` returns `AmrError::Paused` and writes nothing (`personal_nodes_are_sealed_and_a_locked_sealer_writes_nothing`).
## What depends on it
- `/recall` in the cabin, gated by `AppConfig::memory_backend` in `crates/grokhub-app/src/config.rs`.
## Non-obvious
- The JSON key is snake_case `memory_backend` while the rest of `AppConfig` is camelCase. No Settings control exists.
- `AmrStore::at` and `recall` never create directories. `init` makes the tree, and `remember` / `link` also `create_dir_all` their `nodes/` or `edges/` dir, so a new write path can create `amr/` without `init`.
- `recall` is a case-insensitive substring match, sorted by id, capped at 20. A missing `nodes/` dir reads as empty; a bad node file is skipped and the rest still return.
- Scratch is a store flag (`set_scratch`), not a trait method: writes return `AmrError::Scratch`.
- `Sealer` is a trait so grokhub-core stays crypto-free; `LearnedVault` in grokhub-agent implements it. The node id is bound in as AAD, so a renamed `.sealed` file won't open.
- Not synced: do not add `amr/` to hub sync. `forget` is M1+ (a tombstone). Separate from the native engine's sqlite memory in `crates/grokhub-agent/src/memory.rs`.
- The trait doc cites "harness design §9.4"; that design doc is not in `docs/superpowers/specs/`.
## See also
- [grokhub-core](grokhub-core.md), [slash](slash.md), [app-config](app-config.md)

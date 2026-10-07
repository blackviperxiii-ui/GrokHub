# Compass: AMR (agent memory repo, M0–M3, Spike-5a user model)

## Owns
- The `amr/` store under the cabin config dir: `amr/README.md` (`amr_schema: 1`), `nodes/<id>.md`, `nodes/<id>.sealed` (personal or sensitive, Spike-4b), `nodes/<id>.tombstone` (forgotten, M1), `edges/edges.jsonl`, `dreams/import-<date>.md` (M2 import report), `dreams/<date>.md` (M3 nightly dream report), `dreams/forget-<date>.md` (id-only forget notes), and `index.sqlite` (FTS5 over plain nodes, rebuildable; sealed nodes index in memory only).
- Node schema (nine types incl. `routine`, `need`, `mind_prior`; optional `consent_ref` and `sensitivity` lines; five edge relations), `NodeId` validation, the `MemoryEngine` seam (`recall`, `note`, `reflect`, `remember`, `link`, `forget`, `scratch`), and the M1 writers, dual-read and M2 import that run only when `memory_backend` is `amr`. `LegacyMemory` adapts SOUL/USER/MEMORY.
## Quick commands
- `cargo test -p grokhub-core amr` (schema, store, writer, import, dream, user model), `cargo test -p grokhub-agent amr_first_turn` (first-turn dual-read), `cargo test -p grokhub-app dream -- --test-threads=1` (nightly schedule, Halt, legacy, `/memory dream`), `cargo test -p grokhub-app amr -- --test-threads=1` and `cargo test -p grokhub-app recall_` (cabin writers, scratch, forget, import, `/recall`; set `GROKHUB_CONFIG` to a temp dir first)
## Key files
- `crates/grokhub-core/src/amr/mod.rs` (`AmrError`, `MemoryBackend`, `MemoryEngine`, `LegacyMemory`, tests) and `crates/grokhub-core/src/amr/schema.rs` (`NodeType`, `NodeId`, `Node::to_markdown` / `from_markdown`, `EdgeRel`).
- `crates/grokhub-core/src/amr/store.rs`: `AmrStore` (`at`, `with_sealer`, `init`, `recall`, `recall_report`, `remember`, `link`, `edges`, `forget`, `forget_matching`, `revive`, `recallable`, `superseded_ids`, `rebuild_index`, `sealed_index`); `crates/grokhub-core/src/amr/index.rs` (`AmrIndex`). Spike-5a: `crates/grokhub-core/src/amr/signal.rs` (`note_chat`, `detect_correction`, `note_user_edit`, `import_pulse_ledger`, `pulse_ledger_dual`, `note_card_signal`, `note_usage`, `unsafe_to_learn`) and `crates/grokhub-core/src/amr/user_model.rs` (`memory_rows`, `SourceLink`, `edit_node`, `forget_node` with `UserForget`, `strip_forgotten`, `reflect_diff`).
- `crates/grokhub-core/src/amr/write.rs` (`remember_line`: one line, one node, id `mem-<12 hex>`; `sensitivity_for`: PII means sealed) `crates/grokhub-core/src/amr/import.rs` (`import_legacy`, ids `import-<12 hex>`; `durable_chip_prefs`; `write_import_report`), and `crates/grokhub-core/src/amr/dream.rs` (M3 `AmrStore::dream_once`: `supersedes` edges plus tombstones for near-duplicates, tombstones for stale low-confidence nodes, `dreams/<date>.md`; the `DREAM_*` threshold consts; `latest_dream`).
- `crates/grokhub-app/src/app/amr_memory.rs`: cabin glue (`tick_dream` / `dream_tonight` after `tick_review`, `memory_dream_text` for `/memory dream`, `amr_store` runs the one-time import, `amr_remember_now`, `amr_remember_facts`, `run_reflect_amr`, `save_memory_amr`, `forget_amr`, `dual_read_hits`).
- `crates/grokhub-app/src/app/slash.rs`: `recall_amr_lines` and the `/recall`, `/forget`, `/memory note`, native `/remember` branches.
- `crates/grokhub-agent/src/memory.rs`: `first_turn_injection` dual-read (`amr_enabled` reads `app.json`).
## Change recipe
- Read `docs/superpowers/specs/2026-10-07-amr-m0.md` and `docs/superpowers/specs/2026-10-07-amr-m1-m2.md` first.
- New node field: extend `Node`/`NodeDraft`, keep `to_markdown(from_markdown(s)) == s`, and update `schema_round_trip_matches_the_literal_markdown_and_edge_line`.
- New write path: go through `remember_line` (or `AmrStore::remember`), which redacts; mask held secrets first with `redact_held_secrets`; pass the job thread's scratch to `amr_store`. A new signal writer checks `unsafe_to_learn` first and gives every node a non-empty source (`chat:`, `span:`, `pulse:`, `scope:`, `user`).
## What breaks it
- Dream deleting or rewriting anything: it only adds edges and tombstones, never touches USER.md or SOUL.md, and never quotes a sealed node (`dream_merges_duplicates_retires_stale_and_loses_nothing`, `dream_skips_locked_sealed_nodes_and_writes_no_plaintext`). `/dream` is the Imagine prompt (`run_dream`), not this. Also breaking: recalling a superseded node, a forget that stays in `index.sqlite` or the hub snapshot, or a secret, password or one-time code in a node (`forget_leaves_recall_the_index_and_the_hub_snapshot`, `secrets_passwords_and_codes_never_reach_a_node`). Reflect writes nothing; it returns a `ReflectDiff`.
- Writing `amr/` on the legacy path: legacy must never create it (`recall_legacy_finds_memory_line_and_skips_amr`, `legacy_reflect_and_note_never_create_amr`).
- Serializing `memory_backend` when it is legacy: a default `app.json` must not grow the key (`memory_backend_defaults_legacy_and_parses_amr`).
- Ids outside `[a-z0-9-]`, 1..=96 bytes, or starting with `-`; confidence outside 0.0–1.0. A personal node written as `.md`: with no sealer or key, `remember` returns `AmrError::Paused` and writes nothing.
## What depends on it
- `/recall`, History, `/forget <topic>`, reflect, chat insights, `/memory note`, Memory Save (MEMORY.md tab), native `/remember` (gated by `AppConfig::memory_backend` in `crates/grokhub-app/src/config.rs`), and the native first-turn pack (same key, read from `app.json`).
## Non-obvious
- The JSON key is snake_case `memory_backend` while the rest of `AppConfig` is camelCase. No Settings control exists.
- `AmrStore::at` and `recall` never create directories; `init`, `remember` and `link` do. `recall` is a case-insensitive substring match, sorted by id, capped at 20; a bad node file is skipped.
- Scratch is a store flag (`set_scratch`): writes and forgets return `AmrError::Scratch`. A tombstone is a sidecar file, so it works on sealed nodes; reflect never lifts one, an explicit remember does (`revive`).
- `Sealer` is a trait so grokhub-core stays crypto-free; `LearnedVault` in grokhub-agent implements it, with the node id as AAD.
- Dream compares `updated` / `created` as strings against RFC 3339 cutoffs (keep `unix_ms_to_rfc3339` stamps). A node with any edge is left alone, except an earlier winner (only outgoing `supersedes`). A same-day rerun that changes nothing keeps that day's report byte for byte.
- Not synced: do not add `amr/` to hub sync. Separate from the native engine's sqlite memory in `crates/grokhub-agent/src/memory.rs`.
- `poll_reflect` reads an empty `MemoryEdit::next` with a diff as an AMR reflect. AMR `/recall` reads learning.json only for `habit:` and `skip:` lines; the rest were imported.
## See also
- [grokhub-core](grokhub-core.md), [slash](slash.md), [app-config](app-config.md)

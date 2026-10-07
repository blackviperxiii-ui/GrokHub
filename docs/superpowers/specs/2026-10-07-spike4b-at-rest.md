# Spike-4b trust floor: at rest, recall masking, scope rows (2026-10-07)

Second slice of Spike-4 (privacy and consent), an addendum to `docs/superpowers/specs/2026-10-07-spike4a-trust-floor.md`. `harness::decide` is unchanged: same steps, same outcomes, same hard cards (no Always, Enter does not approve, Esc or 5 min timeout denies). No version bump, no new crate, no new network call.

## What is sealed

| File | 4b |
|---|---|
| `consent.jsonl` | sealed, one line per grant or revoke |
| `egress.jsonl`, `egress.1.jsonl` | sealed, one line per send |
| `amr/nodes/<id>.sealed` (personal or sensitive notes) | sealed, whole note |
| `SOUL.md`, `USER.md`, `MEMORY.md`, plain AMR nodes, `amr/edges/edges.jsonl` (ids only) | plain on purpose (the curated tier stays `cat`-able) |
| `learning.json`, spans, `hub-state.json`, native `index.sqlite` | plain for now (see Open) |

Scope indexes don't exist yet (Spike-8), so there is nothing to seal there. Nothing in the cabin writes a personal AMR note yet either; `Sensitivity` and the sealed tier are the frame Spike-5 writes into.

## Format and key

A sealed line is `gh-sealed:v1:` + base64(nonce 12 ‖ ciphertext ‖ tag 16), ChaCha20-Poly1305 via `ring` with a fresh random nonce per line. The AAD names the file kind (`grokhub:consent:v1`, `grokhub:egress:v1`) or the note (`grokhub:amr-node:v1:<id>`), so a line moved between files or a note renamed onto another id doesn't open. A tampered, truncated or wrong-key line doesn't open and counts as unreadable; it is never deleted.

The key is 32 random bytes, base64, in the OS keyring under service `GrokHub`, account `learned-tier-key` (`keyring` 4: Secret Service, Credential Manager, Keychain). It is made on the first sealed write, only when no key id is recorded and nothing here is sealed, under a `create_new` lock file, then read back before use. It is never overwritten. `learned-key.id` (0600) holds the first 8 bytes of a SHA-256 over a label and the key, so a different key is told apart from a missing one. Key bytes sit in `Zeroizing` buffers and never reach a log, span, ledger, status line or `Debug`.

Only `grokhub` (`main`) calls `use_os_keyring`. Every other process and every test defaults to a store with no key, so no test touches a real keyring; tests register a `MemoryKeyStore`.

## Fail closed

`Locked` is `Unavailable` (keyring didn't answer), `Missing` (sealed data or a key id but no key), `WrongKey`, `Busy` (another window is making the key) or `Unwritable` (disk). Then:

- The ledger reads as locked: no grant applies, a write or revoke is refused, and nothing is written. That includes a legacy plain-text ledger: with no keyring, learned data is not read at all.
- `guard_egress`: a grant send whose log line can't be written is refused (`not sent: …`). Model-host and public calls still go, unlogged. Approve-once `/sync` can't be logged, so it isn't sent. The cabin checks first and posts `Not synced. <message>` without a card.
- AMR: `remember` of a personal note returns `AmrError::Paused` and creates no file; `recall_report` skips sealed notes and counts them, and `/recall` says how many weren't searched.
- Settings → Permissions (once, at the top, above every row it holds), `/privacy` and `/recall` show `Locked::message()`. It names the OS store, says nothing is saved as plain text, and for a missing or wrong key says nothing was deleted.

A UI read never waits on the keyring: `ConsentLedger::load_now` returns `pending` and asks in the background. A "keyring down" or "no key" answer is reused for 30 s before the keyring is asked again. A key that opened stays in memory for reads; a write re-asks the keyring once that answer is 30 s old, so a key removed from the keyring stops new sealing. Settings asks for the lock state at most once a second while it paints, so a locked config doesn't re-read the files every frame.

## Migration

A file with no sealed line is a legacy file. When the keyring answers, it is read as plain text exactly as before. The first sealed write seals every existing line in place (same order, 0600 temp file and rename), then appends. A plain-text line found in a sealed file is never trusted (a forged grant can't be slipped in) and is counted as unreadable. A revoke is sticky: a replayed grant line can't undo it.

## Recall masking

`redact_recall` = `redact_secrets` + `redact_pii`, applied to each snippet in `memory::first_turn_injection` (the native engine's recall pack, the one recall pack that reaches a model prompt). Masks `[email]`, `[phone]`, `[card]` (IIN prefix + Luhn), `[ssn]` (`ddd-dd-dddd`, valid area and group) and `[address]` (number + Capitalized street + suffix, PO Box). Hand-rolled, deterministic, word boundaries on both sides; file-extension TLDs, `git@` remotes, versions, IPs, hashes, timestamps and identifiers are left alone (tested). The user's own `/recall` and Memory views are not masked.

## Scope rows

Settings → Permissions → "What GrokHub can read", after "Leaving this computer": one row per 4a scope, off by default, with a plain hint. Allow takes a pointer click only (`cards::settings_grant_row`, `clicked_by(Primary)`); the hub Allow uses it too. Revoke is a ghost. A refused Allow says why under the heading, since the status line sits behind the modal. Files take a typed folder (home itself, any folder that holds it, relative and excluded paths are refused); browser history takes a browser from a list. `/privacy` lists scope grants by name and its Revoke works for them. Nothing reads a scope yet.

## Polish (SB-01 to SB-11)

Critiquito's follow-ups after #525. `decide`, `Locked`'s fail-closed handling and the hard-card rules are unchanged.

- While locked, every Settings grant row is disabled (egui's disabled look) with `lock_hover` on hover; Revoke too, since `revoke_grant` refuses a locked ledger. The `/privacy` Revoke rows take the same lock.
- The lock gets one next step per OS (`KeyringOs`, chosen with `cfg`) and a Try again that calls `recheck_keyring` (drops the cached keyring answer, nothing else) and re-reads the ledger. While the keyring hasn't answered, the last lock stays on screen. `/sync` and `/privacy` add the short form.
- Folder grants are titled by folder name; the path is in the hint (middle ellipsis) and on hover. Several folders, one grant each. Choose folder… uses `rfd` (xdg-portal only on Linux) off the UI thread (on the main thread on macOS) and only fills the field. A `None` within 400 ms means no dialog opened, so the row says to type the path.
- `/privacy` lists each grant once under one Grants heading, then "Off: …".

## Open

- `learning.json` (legacy learned state) stays plain. The design moves learning into AMR in Spike-5; sealing it now would pause legacy learning on every machine without a keyring.
- No keyring on a machine means the hub grant stops applying and `/sync` can't send there, even for a ledger written before 4b. That follows "fail closed"; the alternative is to keep reading an all-plain legacy ledger without a keyring.
- Native-turn egress lines still record `redactions: 0`; the recall-pack count isn't threaded through yet.
- Real keyrings were exercised on Linux (gnome-keyring) only. Windows Credential Manager and macOS Keychain are built (the `keyring` crate already ships in the cabin for Imagine) but not run.

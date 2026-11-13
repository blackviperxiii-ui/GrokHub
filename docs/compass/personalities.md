# Compass: personalities, dynamic chat and the quip bank

## Owns
- The built-in voices (`crates/grokhub-core/assets/personalities.toml`), custom presets, the 18+ gate and the heat dial (`personality.rs`).
- Dynamic chat (`voice_flow.rs`), the quip bank and its timing (`quips.rs`, `crates/grokhub-core/assets/quips.toml`) and the per-chat session that runs both (`crates/grokhub-agent/src/voice_session.rs`).

## Quick commands
- `cargo test --locked -p grokhub-core --lib personality` (also `voice_flow`, `quips`); agent: `voice_session`
- `cargo test --locked -p grokhub-app --bins personality wardrobe`

## Key files
- `crates/grokhub-core/src/personality.rs`: `Heat`, `prompt_block_for`, `ADULT_HUMOR`, `RETIRED`, `find`.
- `crates/grokhub-core/src/voice_flow.rs`: `Flow::step` with momentum, two wins to switch and a minimum stay; `voice_for` never returns an explicit voice.
- `crates/grokhub-core/src/quips.rs`: `Planner::step`, `Trigger::detect`, `nudge`.
- `crates/grokhub-agent/src/voice_session.rs`: `register`, `step` (called from `write_brief`), `turn_block` (read by the run into the volatile block).
- the retired cabin's personality_ui.rs: Settings → Personality, chat_voice_config, open_chat_guest_voice.

## Change recipe
- A new built-in voice: add it to `personalities.toml` with `topics`, then the wardrobe tables in `companion/wardrobe.rs` and `loadout_home/nyx_wardrobe.rs` (or `AWAITING_ART` until its art lands), and the counts in both tests.
- Retiring a voice: add it to `RETIRED` with the voice it speaks as; move its poses into Nyx's mood extras so the pose count holds.
- A new quip: add it to `quips.toml`; the tests check every line against the guard and the mean-words list.

## What breaks it
- Per-turn text in the cached system prefix: the guest voice, depth note and quip slot belong in the volatile block (`TurnVolatile::with_voice`).
- Tagging Cheeky as 18+: only Spicy and Filthy add `ADULT_VOICE`, so a mild chat can still route anywhere.
- Trusting the 18+ marker in typed text: `app_note_tags_adult` only reads it from the app's own volatile note.

## What depends on it
- Routing in `crates/grokhub-core/src/adult_guard.rs` (`explicit_conversation`), the Nyx card art, the live eval cases `adult-ask-no-lecture` and `filthy-roast-lands`.

## Non-obvious
- 18+ is never inferred from chat; only the switch and its one-time check turn it on, and heat nudges in chat do nothing while it is off.

## See also
- [grokhub-core](grokhub-core.md), [grokhub-agent](grokhub-agent.md), app-module

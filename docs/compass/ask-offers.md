# Compass: repeated-ask automate offers

## Owns
- Local clustering of the user's own typed asks by intent (`same_topic`) and hour, then one Pulse Automate / Skill offer after `OFFER_MIN_COUNT` hits.
- Pure rules in `crates/grokhub-core/src/ask_offers.rs`; cabin log + tick + dismiss in `ask_offers_ui.rs`. Asks stay in `{config}/ask-offers.json`.

## Quick commands
- `cargo test -p grokhub-core ask_offers --locked -- --test-threads=1`
- `GROKHUB_CONFIG=$(mktemp -d) cargo test -p grokhub-app ask_offer -- --test-threads=1`

## Key files
- `crates/grokhub-core/src/ask_offers.rs`: `append_ask`, `cluster_asks`, `offer_preview`, `build_offer_card`, `tick`, `dismiss_cluster`.
- `crates/grokhub-app/src/app/ask_offers_ui.rs`: `note_ask_offer`, `tick_ask_offers`, `ask_offer_dismissed`.
- Hooks: `chat_ui.rs` (`send_from_composer`), `feed_ui.rs` (`accept_automate_offer`), `pulse_ui.rs` (`pulse_dismiss`).

## Change recipe
- Threshold or copy: edit core constants / `suggestion_title` / `offer_preview` with fixture tests.
- Keep the schedule seed in `card.prompt`; Accept uses `automate_accept_seed`. Settings: `AppConfig.ask_offers`.

## What breaks it
- Human title without a `prompt`: Accept opens Automations and saves nothing.
- Skipping `dismiss_cluster` on Dismiss: the cluster can return after `last_offered` clears.

## What depends on it
- Pulse Automate Accept (`commit_schedule`) and Skill Apply (`apply_idea`).
- `same_topic` / `is_one_off_ask` (`ideas.rs`); `parse_nl_automation` (`automation.rs`).

## Non-obvious
- Peak hour ±1 must hit `OFFER_MIN_COUNT` for Automate; scattered hours become a Skill idea.
- Dismiss is permanent for that cluster key. `tick_ask_offers` runs from `tick_feed_pulse`.

## See also
- [nudge-watchers](nudge-watchers.md), [heartbeat](heartbeat.md), [app-module](app-module.md)

# Compass: voice mode (Hey Grok in Loadout)

## Owns
- Open mic, turn end, streaming Smart Turn STT, spoken sentence playback, barge-in, and shared source selection across STT, reply model, and TTS.
- Core state in `crates/grokhub-core/src/voice_turn.rs`, `crates/grokhub-core/src/stt_stream.rs`, `crates/grokhub-core/src/voice.rs`; routing in `crates/grokhub-agent/src/route/voice_route.rs`; shell audio in `src-tauri/src/voice.rs`, `src-tauri/src/stt_stream.rs`, `src-tauri/src/voice_live.rs`; UI in `VoiceMode.tsx` and `VoiceRouteSettings.tsx`.

## Quick commands
- `cargo test --locked -p grokhub-core voice_turn stt_stream`
- `cargo test --locked -p grokhub-agent voice`
- `cargo test --locked --manifest-path src-tauri/Cargo.toml voice`

## Key files
- `crates/grokhub-core/src/voice_turn.rs`: `TurnDetector` (noise floor, pre-roll, `set_playing`) and `SpeechChunker` (sentences, hides thinking and commands).
- `crates/grokhub-core/src/stt_stream.rs`: socket URL, `parse_stt_event`, `SttTurn`.
- `crates/grokhub-core/src/voice.rs`: `stt_url`, `tts_url`, `tts_voice`, `realtime_can_connect`.
- `crates/grokhub-agent/src/route/voice_route.rs`: `VoiceRouteMode` (`Auto` / `LocalOnly` / `Pinned`), `VoicePin`, `plan_stage`, 5-minute xAI cooldown, and `active_reply_pref` (`voice-route.json`).

## Change recipe
- Turn timing: change constants in `crates/grokhub-core/src/voice_turn.rs` and `crates/grokhub-core/src/voice_turn_tests.rs`.
- A new stream event: add it to `SttEvent`, map it in `SttTurn::apply`, test the JSON shape.
- STT/reply/TTS source routing: update `voice_route::plan_stage` and `active_reply_pref`; keep xAI OAuth bearer (`realtime_bearer_with`) ahead of saved API key and never hardcode online-first order.

## What breaks it
- Sending audio before `transcript.created` or sending reply echo to STT.
- Changing HTTP agent counts checked by `offline_guard.rs` (`voice.rs=1`, `voice_live.rs=1`, `stt_stream.rs=2`), bypassing `plan_stage`, or retrying after 401/403 (`REFUSED`).

## What depends on it
- Live chat reply streaming and voice playback (VoiceMode.tsx), Settings → Local AI (VoiceRouteSettings.tsx), and active-session reply routing in `crates/grokhub-agent/src/route/live.rs`.

## Non-obvious
- Smart Turn can only end a turn early; the mic's detector ends it at the latest, then waits up to 2.5 s. A long reply resets the quiet clock.
- VoiceSession subscribes to model-library events while active to report local Parakeet/Kokoro background downloads without spamming progress ticks.

## See also
- [grokhub-core](grokhub-core.md), [router](router.md)

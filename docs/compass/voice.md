# Compass: voice mode (Hey Grok in Loadout)

## Owns
- Open mic, Smart Turn STT, spoken replies, barge-in, shared STT/reply/TTS routing, and opt-in on-device wake word (Order 970).
- Core: `crates/grokhub-core/src/voice_turn.rs`, `stt_stream.rs`, `voice.rs`, `wake_word.rs`; agent `crates/grokhub-agent/src/route/voice_route.rs`; shell `src-tauri/src/voice.rs`, `stt_stream.rs`, `voice_live.rs`, `wake_word.rs`; UI `VoiceMode.tsx`, `VoiceRouteSettings.tsx`, `WakeWordSettings.tsx`.

## Quick commands
- `cargo test --locked -p grokhub-core voice_turn stt_stream wake_word`
- `cargo test --locked -p grokhub-agent voice`
- `cargo test --locked --manifest-path src-tauri/Cargo.toml voice wake_word`

## Key files
- `crates/grokhub-core/src/voice_turn.rs`: `TurnDetector` and `SpeechChunker`.
- `crates/grokhub-core/src/wake_word.rs`: `WakeListener`, mic opt-in, open-vocabulary phrases, profile routes, hands-free stop; `feed_local` only.
- `crates/grokhub-agent/src/route/voice_route.rs`: `VoiceRouteMode` (`Auto` / `LocalOnly`), `plan_stage`, xAI cooldown, `voice-route.json`.
- `src-tauri/src/voice_live.rs`: open mic; pauses wake matching while the session is active.

## Change recipe
- Turn timing: constants in `voice_turn.rs` / `voice_turn_tests.rs`. New STT event: `SttEvent` + `SttTurn::apply`.
- Wake phrases/routes: edit `wake_word.rs` + Settings → Local AI (`WakeWordSettings.tsx`); never send wake audio to the cloud.
- STT/reply/TTS routing: `plan_stage` / `active_reply_pref`; OAuth bearer ahead of API key.

## What breaks it
- Audio before `transcript.created`, reply echo to STT, or cloud STT into wake `feed_local`.
- HTTP agent counts in `offline_guard.rs` (`voice.rs=1`, `voice_live.rs=1`, `stt_stream.rs=2`), bypassing `plan_stage`, or retrying 401/403.

## What depends on it
- VoiceMode playback, Local AI voice + wake settings, `crates/grokhub-agent/src/route/live.rs` reply routing.

## Non-obvious
- Smart Turn ends early; mic detector is the latest bound; long replies reset the quiet clock. Wake refuses with `VOICE_BUSY` while open-mic owns the mic.
- VoiceSession watches model-library events for Parakeet/Kokoro downloads without progress spam.

## See also
- [grokhub-core](grokhub-core.md), [router](router.md), [personalities](personalities.md)

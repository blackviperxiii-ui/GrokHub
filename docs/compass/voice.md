# Compass: voice mode (Hey Grok in the cabin)

## Owns
- One open mic per voice session, end of turn, streaming speech-to-text with Smart Turn, spoken replies sentence by sentence, and barge-in.
- Pure parts in core (`voice_turn.rs`, `stt_stream.rs`, `voice.rs`); sockets, threads and playback in the app (`voice_mic.rs`, `stt_stream.rs`, `spoken.rs`, `app/voice.rs`).

## Quick commands
- `cargo test --locked -p grokhub-core voice_turn stt_stream`
- `cargo test --locked -p grokhub-app voice_mic stream_tests spoken`

## Key files
- `crates/grokhub-core/src/voice_turn.rs`: `TurnDetector` (noise floor, pre-roll, echo-aware `set_playing`) and `SpeechChunker` (sentences, hides thinking and commands).
- `crates/grokhub-core/src/stt_stream.rs`: the socket URL, `parse_stt_event`, `SttTurn` (how a turn's words come together).
- `crates/grokhub-core/src/voice.rs`: the xAI speech-to-text and text-to-speech calls (`stt_url`, `tts_url`, `tts_voice`).
- The open-mic loop (capture, one socket per listen, spoken replies) lived in the retired cabin; the Tauri app has push to talk (src-tauri/src/voice.rs) but no open-mic voice mode yet.

## Change recipe
- Turn timing: change the constants in `voice_turn.rs` and its tape tests (`voice_turn_tests.rs`), never the app loop.
- A new stream event: add it to `SttEvent`, map it in `SttTurn::apply`, test the documented JSON shape.
- Anything that starts a listen goes through start_ptt_listen, which decides over-reply listening from voice_hold_rx.

## What breaks it
- Sending audio before `transcript.created`, or sending the reply's own echo to be transcribed (keep the held blocks).
- Opening the stream behind a proxy (behind_proxy) or retrying after a 401/403 (`REFUSED`).
- Cloning a whole reply onto the UI thread in `speak_reply` (a source test pins it).

## What depends on it
- JobOut::Voice, JobOut::VoiceNote and JobOut::VoiceHeard in `app/jobs.rs`; speak_stream from ingest_stream_chunk; `VOICE_TALK_STYLE` from `chat_kick.rs`.

## Non-obvious
- Smart Turn can only end a turn early; the mic's detector still ends it at the latest, then final_words waits up to 2.5 s. A long reply resets the quiet clock.

## See also
- app-module, [grokhub-core](grokhub-core.md)

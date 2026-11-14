---
name: av-qa-walkthrough
description: AV ship gate. Playback, sync, codecs, devices; ffprobe/ffmpeg; ship/no-ship. /av-qa-walkthrough
---

# AV QA Walkthrough

AV ship gate before beta→main or after pipeline/player/codec changes. UI → **qa-walkthrough**. Cite **viper-mode**; no sibling skill growth. **not hand-tested on device** if hardware skipped.

## 1. Playback walk

Real hardware when possible. Transport, loop/gapless, first-frame/audio latency; no clicks/pops/stutter; soak. Sync: clap/flash/timecode ms; post-seek; drift. Quality: res/fps, frames, bitrate, artifacts; `ebur128`, clip, channels, rate. mp4/mkv/webm/mov/wav/flac/mp3/ogg; h264/h265/av1/vp9/aac/opus; HDR/SDR/VFR; bad media fails clean. Outputs: speakers/phones/HDMI/BT, hot-plug, mute/mixer, mic/cam. Stream/record: encoder, reconnect; `ffprobe`, play-through. No player? Playback = reply/TTS element (first-audio latency, gaps); sync = barge-in and echo: feed the app's own output back in at -10 to -20 dB, no turn fires; codecs = each encode/resample boundary; devices = mic constraints, `devicechange`.

## 2. Code review

Pipeline/clock/sync/device/encoder changes. **principle-fix-root-causes**. Platform first: a native rate or resampler (AudioContext rate, `aresample`) before hand-written DSP; hand-roll only with a reason and measured CPU (**principle-subtract-before-you-add**).

## 3. Security and vulnerabilities

Untrusted media, URL secrets. pii-guard + secret scan; each line names the command run and its result. Blocked per memory = "not attempted"; run the fallback.

## 4. Tests

Suite + build; cover AV paths; flakes = findings. Keep the real-media walk as a rerunnable script, committed with the fix (**principle-build-the-lever**).

## 5. Slop, improvements, optimizations

Deslop; **sharpen** latency and encoders.

## 6. Output

Ranked findings; evidence via `ffprobe -show_streams`, `ebur128`, `blackdetect`, frames. **ship** or **no-ship**. Safe fixes → **beta**; **Needs decision** (on the decision card only; start the recommended reversible option meanwhile); redesigns only.

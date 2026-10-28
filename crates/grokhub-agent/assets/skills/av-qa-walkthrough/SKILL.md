---
name: av-qa-walkthrough
description: AV ship gate. Playback, sync, codecs, devices; ffprobe/ffmpeg; ship/no-ship. /av-qa-walkthrough
disable-model-invocation: true
---

# AV QA Walkthrough

AV ship gate before beta→main or after pipeline/player/codec changes. UI → **qa-walkthrough**. Cite **viper-mode**; no sibling skill growth. **not hand-tested on device** if hardware skipped.

## 1. Playback walk

Real hardware when possible. Transport, loop/gapless, first-frame/audio latency; no clicks/pops/stutter; soak. Sync: clap/flash/timecode ms; post-seek; drift. Quality: res/fps, frames, bitrate, artifacts; `ebur128`, clip, channels, rate. mp4/mkv/webm/mov/wav/flac/mp3/ogg; h264/h265/av1/vp9/aac/opus; HDR/SDR/VFR; bad media fails clean. Outputs: speakers/phones/HDMI/BT, hot-plug, mute/mixer, mic/cam. Stream/record: encoder, reconnect; `ffprobe`, play-through.

## 2. Code review

Pipeline/clock/sync/device/encoder changes. **principle-fix-root-causes**.

## 3. Security and vulnerabilities

Untrusted media, URL secrets. pii-guard + secret scan.

## 4. Tests

Suite + build; cover AV paths; flakes = findings.

## 5. Slop, improvements, optimizations

Deslop; **sharpen** latency and encoders.

## 6. Output

Ranked findings; evidence via `ffprobe -show_streams`, `ebur128`, `blackdetect`, frames. **ship** or **no-ship**. Safe fixes → **beta**; **Needs decision**; redesigns only.

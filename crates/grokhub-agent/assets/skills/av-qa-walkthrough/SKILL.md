---
name: av-qa-walkthrough
description: AV ship gate. Playback, sync, codecs, devices; ffprobe/ffmpeg; ship/no-ship. /av-qa-walkthrough
---

# AV QA Walkthrough

The ship gate for audio and video. Use it before promoting beta to main, or after any change to a pipeline, player, codec, clock or device path. UI and general code go to **qa-walkthrough**. Cite **viper-mode**.

Ears and eyes miss what a measurement catches, and a measurement misses what a person hears. Do both. Every claim carries a measured value and the command that produced it. If you could not test on real hardware, the report says **not hand-tested on device** in its first line, and names which checks that leaves open.

## 0. Scope and setup

1. State what changed and which stages it touches: capture, decode, resample, mix, encode, mux, transport, render.
2. Gather test media before you start: a known-good file per container and codec the app supports, a sync test clip (clap, flash or burned-in timecode), a long file for the soak, and deliberately bad files (truncated, zero-length, wrong extension, corrupt header).
3. Probe each file first so you know what you are feeding in: `ffprobe -v error -show_format -show_streams <file>`.

## 1. Playback walk

Use real hardware when possible.

**Transport.** Play, pause, stop, seek forward and back, scrub, rate change, loop and gapless transitions. Measure first-frame and first-audio latency. Listen for clicks, pops and stutter at every transition, since those are where they hide. Run a soak (at least 30 minutes, or the longest session a user would have) and watch memory, CPU and drift.

**Sync.** Measure audio against video in milliseconds with the clap, flash or timecode clip. Measure again after a seek, after a pause and resume, and at the end of the soak to catch drift. State the tolerance you are holding (lip sync is usually judged within about 40 ms) and whether it held.

**Quality.** Check resolution and frame rate against the source, dropped or duplicated frames, bitrate and visible artifacts. For audio, check loudness with `ebur128`, clipping, channel count and order, and sample rate.

**Formats.** Cover each the app claims: mp4, mkv, webm, mov, wav, flac, mp3, ogg; h264, h265, av1, vp9, aac, opus; HDR and SDR; variable frame rate. Bad media must fail cleanly, with a message and no hang, crash or stuck state.

**Outputs and devices.** Speakers, headphones, HDMI and Bluetooth. Hot-plug and unplug during playback. Mute and mixer changes. Microphone and camera permission, selection and loss.

**Streaming and recording.** Encoder settings match what is configured. Drop the network and confirm reconnect. Probe the recorded or streamed output with `ffprobe`, then play it through start to finish.

**No player?** A voice or reply app still has every stage. Playback is the reply or TTS element: measure first-audio latency and gaps. Sync is barge-in and echo: feed the app's own output back into its input at -10 to -20 dB and confirm no turn fires. Codecs are each encode and resample boundary. Devices are microphone constraints and `devicechange`.

## 2. Code review

Read every change to the pipeline, clock, sync, device or encoder code, then its neighbors. Root-cause each defect with **principle-fix-root-causes**.

Prefer the platform. Use a native rate or resampler (the `AudioContext` rate, `aresample`) before hand-written DSP. Hand-roll only with a stated reason and a measured CPU cost (**principle-subtract-before-you-add**).

Look for the usual AV faults: two clocks treated as one, timestamps assumed monotonic, buffers sized for one sample rate, work on the audio or render thread that can block, and device changes handled only at startup.

## 3. Security and vulnerabilities

Treat every media file and stream as untrusted input: malformed containers, huge dimensions or durations, and embedded metadata. Check that URLs carrying tokens or keys never reach logs. Run the repo's pii-guard and secret scan.

Each line of this section names the command run and its result. If a check is blocked by a standing instruction or recorded limit, write "not attempted" and run the fallback. Do not hunt for a way around the block.

## 4. Tests

Run the suite and the real build. Cover the AV paths the change touches. A flaky test is a finding.

Keep the real-media walk as a rerunnable script, committed with the fix, so the next person repeats it with one command (**principle-build-the-lever**).

## 5. Slop, improvements, optimizations

Remove slop from the changed code. Note latency and encoder improvements with **sharpen**. Measured performance work goes to **optimize**.

## 6. Output

Findings ranked critical, high, medium and low, each with the evidence that shows it: `ffprobe -show_streams` output, `ebur128` readings, `blackdetect` or `silencedetect` hits, frame counts, measured milliseconds. List what was not tested and why.

End with **ship** or **no-ship** and the blockers.

- A clear, safe fix goes in a small PR into **beta** with its test.
- A finding that needs a human decision goes under **Needs decision**, with options and a recommendation, and is raised on the decision card only. Start the recommended option meanwhile when it is reversible.
- Redesigns go in the report only.

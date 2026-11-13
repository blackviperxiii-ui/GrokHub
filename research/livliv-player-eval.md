# LivLiv Player evaluation (`#1511`)

Evaluation of [`livliv-dev/player`](https://github.com/livliv-dev/player) (`@livliv-dev/player` `0.5.0`) as a candidate engine for a live, TTS lip-synced Nyx in Loadout (`blackviperxiii-ui/GrokHub`), either replacing or complementing the 144 pre-rendered Nyx clips (`assets-nyx-anim-1`).

Date: 2026-11-13
Scope: Research/evaluation only (no dependency added or runtime code modified).

---

## Summary verdict

**Conditional Yes on a small 2–3 day spike, scoped strictly to 1–2 hero speaking surfaces (`NyxDock` and `NyxDesk`) — and Do Not replace the 144 `<NyxCard>` mood/action clips.**

`@livliv-dev/player` has a clean Apache-2.0 core (~33.6 KB gzipped, zero runtime deps), a Web Audio TTS queue (`@livliv-dev/player/voice`) that aligns well with `VoiceSession` (`ui/src/VoiceMode.tsx`), and explicit React StrictMode cleanup semantics. The blockers are **art format** and **WebView verification**, not the JS library itself:

1. All 144 bundled Nyx images (`448×350` flattened RGB JPEGs with baked backgrounds and varied scene poses) cannot be turned into 2.5D `.livliv` v4 layered rigs directly, nor do 144 action scenes make sense to rig individually.
2. Upstream `API.md` explicitly notes desktop browser coverage only (*"not yet on phones or in in-app WebViews"*), so WebGL2 stability in Windows WebView2, Linux WebKitGTK, and Android WebView must be tested before committing.
3. Upstream's 21 bundled sample characters are **not** Apache-2.0 (they carry non-commercial and mascot restrictions), so zero upstream sample assets can ship with Loadout.

A short spike should test **both** (A) `@livliv-dev/player` / `PngBody` with one hero Nyx asset and (B) a tiny ~3 KB Web Audio bandpass envelope driving the `MouthPatches` (`closed` / `half` / `open`) and `eyes` overlay schema that already exists in `crates/grokhub-core/src/companion.rs`.

---

## 1. License check

| Component | License | Ship-safe in Loadout (MIT)? | Notes / Obligations |
|---|---|---|---|
| `@livliv-dev/player` code (`src/`, ESM bundle) | **Apache-2.0** | **Yes** | Listed in `deny.toml` (`Apache-2.0` allowed); compatible with GrokHub's `MIT` `LICENSE`. Must retain `LICENSE` and `NOTICE` (`ui/vite.config.ts` already emits `licenses.md` on build). |
| Live2D / Cubism dependencies | None used | **Yes** | Uses an open JSON + WebP `.livliv` format (`FORMAT.md` v4 / v3). No proprietary Cubism Core SDK or `.moc3` runtime. |
| `@livliv-dev/player/vrm` peer deps (`three`, `@pixiv/three-vrm`) | MIT | Yes (unneeded) | Only pulled if instantiating `VrmBody`; 2.5D `Player` and 2D `PngBody` do not import them. |
| `@livliv-dev/player/tracking` (`@mediapipe/tasks-vision`) | Apache-2.0 (WASM + TFLite) | Caution (privacy/size) | Pulls ~8 MB WASM/model assets from jsDelivr/GCS by default and makes periodic telemetry requests unless self-hosted and isolated. Not needed for TTS lip-sync. |
| Upstream `characters/*` (21 sample rigs) | **`LicenseRef-LivLiv-Samples`** (most), **CC BY-NC-SA 4.0** (`p01`, `p02`), none (`p03`) | **No — Do Not Ship** | `characters/LICENSE.md` and `characters/CREDITS.md` restrict sample rigs to non-commercial personal use and prohibit commercial product promotion; `d03`/`d03ft` also carry mascot restrictions, and `p03` grants no rights. |
| `livliv.ai` hosted auto-rig exports | `LicenseRef-LivLiv-Terms` | Depends on tier | Free-tier exports set `meta.watermark: { "kind": "livliv" }`, which the renderer draws onto the canvas as a license condition (`FORMAT.md` §3). Clean shippable rigs require either hand-authoring in `.livliv` v4 open format or a watermark-free plan (`watermark: "none"`). |

**Net takeaway:** The player SDK is legally clean (`Apache-2.0`, already compatible with our Vite license bundling), provided zero files from upstream `characters/` are committed or redistributed and any `.livliv` Nyx rig we create either uses our own clean v4 pipeline or a watermark-free export.

---

## 2. Fit with the GrokHub / Loadout stack

GrokHub's desktop shell (`src-tauri/`) runs **Tauri 2 + React 19 + TypeScript 7 + Vite 8** (`ui/package.json`), with the same UI also served to paired phones/browsers via `src-tauri/src/remote.rs` and packaged for Android.

### Runtime & lifecycle fit

- **Module & TypeScript compatibility:** Pure ESM (`"type": "module"`, `"sideEffects": false`, ES2023 target), typed exports across subpath entrypoints (`@livliv-dev/player`, `/voice`, `/png`, `/body`, `/tracking`).
- **React 19 StrictMode:** `API.md` documents the exact StrictMode mount/unmount recipe (`new Player(canvas)` in an effect with `player.dispose({ release: true })` cleanup so double-mount does not leak a WebGL context).
- **Multi-surface WebGL budget:** Browsers cap active WebGL2 contexts (~8–16 depending on driver/GPU). Mounting live 2.5D canvases simultaneously across `NyxDock`, `Home`, `Personality`, and the separate `nyx-desk` transparent window (`NyxDesk.tsx`) would burn multiple contexts. Only active visible speech targets (`NyxDock` when docked, or `NyxDesk` when undocked) should own a live player; all gallery/card grids should keep `<NyxCard>` (still JPG + streamed video).
- **Unverified WebViews:** `API.md` explicitly states: *"Tested on current desktop Chromium, Firefox and WebKit; not yet on phones or in in-app WebViews."* Tauri runs on **WebView2** (Windows), **WebKitGTK** (Linux), and **Android WebView** — transparency (`alpha: true` WebGL compositing inside the click-through `NyxDesk` window), `createImageBitmap`, and hardware WebGL2 context loss under minimize/sleep need real smoke testing on Windows and Linux. `Player.isSupported()` provides a clean fallback hook to `<NyxCard>`.

### Voice & TTS integration (`VoiceMode.tsx` & `voice_live.rs`)

`src-tauri/src/voice_live.rs` streams reply chunks as `Spoken { mime: "audio/mpeg" | "audio/wav", audio: base64 }`, queued by `VoiceSession` in `ui/src/VoiceMode.tsx`. Right now `playSpoken()` creates a `new Audio("data:...")` element per chunk.

LivLiv offers two lip-sync hooks in `@livliv-dev/player/voice`:

1. **`createSpeech(player)` (`SpeechPlayer`):** Decodes MP3/WAV into Web Audio, measures a 100 fps speech-band RMS envelope (150–1500 Hz, 30 ms acoustic lead, plus round/wide vowel formants), and schedules gapless `AudioBufferSourceNode` playback synced to `AudioContext.getOutputTimestamp()` (even compensating for Bluetooth latency). Can replace `playSpoken()` inside `VoiceSession` cleanly (`onStart` / `onEnd` hooks map to `Playing.done`, `stop()` maps to barge-in).
2. **`startLipSync(player, audioEl)` (`VoiceMeter`):** Taps an existing `<audio>` element via `MediaElementAudioSourceNode`. Simpler glue around existing `playSpoken()`, though Web Audio allows `createMediaElementSource()` only once per element and misses `SpeechPlayer`'s ahead-of-sound lookahead + vowel tracking.

> **Voice mode mic echo note:** Replacing `<audio>` with `AudioContext` output (`SpeechPlayer`) should be tested alongside `openBrowserMic()` (`ui/src/VoiceMode.tsx`) on Windows/Linux WebViews to confirm hardware AEC (acoustic echo cancellation) still cancels Web Audio output cleanly.

### CSP & `PRIVACY.md` compatibility

GrokHub enforces strict offline-by-default CSP in both `src-tauri/tauri.conf.json` and `src-tauri/src/remote.rs`:

- Desktop (`tauri.conf.json`): `connect-src ipc: http://ipc.localhost`
- Remote UI (`remote.rs`): `connect-src 'self'`

Two concrete CSP interactions came out of reading LivLiv's source (`src/voice.ts`, `src/runtime/speech.ts`, `src/tracking.ts`):

1. **`SpeechPlayer` `Blob`/`ArrayBuffer` `fetch('blob:...')` trap:** Passing a `Blob` or `ArrayBuffer` to `speech.say(clip)` calls `URL.createObjectURL(...)` and `fetch(url)` internally (`src/voice.ts`, `src/runtime/speech.ts`), which fails under `connect-src ipc: http://ipc.localhost` / `'self'` unless `blob:` is added to `connect-src`. **Clean workaround without relaxing CSP:** decode the base64 `Spoken.audio` chunk into `Uint8Array` and pass `new Response(bytes, { headers: { "content-type": spoken.mime } })` directly to `speech.say(response)` — `Incoming.load()` consumes `Response` streams directly without calling `fetch()`.
2. **Model assets & tracking:** Bundling a `.livliv` directory/package under Vite/Tauri `'self'` makes **zero outbound network requests**, preserving `PRIVACY.md`. By contrast, `@livliv-dev/player/tracking` fetches WASM/models from `cdn.jsdelivr.net` and `storage.googleapis.com` by default and should stay **excluded**.

### Overlap with existing `companion.rs` face-patch schema

`crates/grokhub-core/src/companion.rs` (`Outfit`, `Patch`, `MouthPatches`, `Outfit::animates_face()`) **already defines** an optional per-outfit pixel-anchored `eyes` patch and three-frame `mouth` patch (`closed`, `half`, `open`), though none of the 9 outfits in `crates/grokhub-core/assets/companions/nyx/companion.toml` currently supply patch files and `ui/` does not yet overlay them. Keeping this in mind avoids double-engineering if only 2D mouth movement is desired.

---

## 3. Size & performance

### Code footprint (small)

- **`@livliv-dev/player` core:** ~75 KB minified, **~33.6 KB gzipped** (`API.md` / `package.json`, zero deps).
- **`@livliv-dev/player/voice`:** ~35 KB unminified TS source (`voice.ts`, `runtime/speech.ts`, `runtime/voice.ts`, `runtime/vowel.ts`, `runtime/talk.ts`) -> ~8–10 KB minified/gzipped.
- **`@livliv-dev/player/png` + `/body`:** `png.ts` is ~5 KB unminified source (pure 2D `<canvas>`, zero WebGL, swaps 2–4 pre-decoded `ImageBitmap` sprites with breathing/head-sway/talk-hop).
- **Relative to GrokHub `ui/dist/`:** Total JS chunks on `beta` today are ~1.6 MB uncompressed (`main` ~216 KB, `Settings` ~121 KB, `CodeEditor` ~295 KB, `CodeTerminal` ~329 KB); adding core + voice adds ~43 KB gzipped (~3–4% of main JS), tiny compared to WebGL-free video streaming.

### Asset footprint (the real weight)

| Approach | Asset structure | Typical size per outfit | Notes |
|---|---|---|---|
| **Current `NyxCard` clips (PR `#1898`)** | Bundled `448×350` JPG poster + streamed `.webm`/`.mp4` on GitHub release | ~25–35 KB bundled per pose (~4 MB total for 144 JPGs) | Zero extra app installer bloat for motion clips; degrades gracefully offline to JPGs. |
| **`PngBody` (`@livliv-dev/player/png`)** | 2–4 sprites (`closed`, `open`, optional `blink`, `blink-open`) | ~80–250 KB per pose | Works on 2D Canvas; trivial offline/WebView compat; no eye tracking/parallax. |
| **Full `.livliv` v4 (`Player`)** | 30–70 separated WebP texture layers (`model.json` + `textures/*.webp` + optional `keyforms.json`) | **~0.7–4+ MB per outfit** | Upstream sample `k01` has 40 WebPs (~700 KB); refined `d03ft` `keyforms.json` alone is **3.10 MB**. Packaging 10–144 `.livliv` outfits locally is prohibitive for app bundle size; realistic only for 1–2 hero outfits. |

### CPU / GPU runtime cost

- Idle + audio envelope (`envelope()` runs once per decoded TTS chunk at 100 Hz) uses negligible CPU (<1 ms per chunk).
- WebGL2 2.5D rendering draws 30–70 alpha-blended meshes up to `dpr <= 2` (`maxPixelRatio: 1` cuts fill-rate 4× on 200% DPI screens and should be used for corner dock widgets `~220 px` wide).
- Hidden canvases (`renderWhenHidden: false` default) pause the RAF loop automatically when off-screen or minimized.

---

## 4. Can our Nyx art be rigged for it?

**No, not automatically from the existing 144 shipped assets — and rigging all 144 is the wrong model anyway.**

### Audit of existing Nyx assets

- **Mood/action catalog (`144` images under `brand/nyx-*.jpg` and `brand/nyx/**`):** Every file is a flattened `448×350` landscape RGB JPEG with a **baked-in scene background** (desk, monitors, headset, stethoscope, keyboard, clipboard, varying three-quarter/side/seated camera angles). Face width is typically only ~80–120 px.
- **Desktop companion figure (`brand/companion.png`):** `351×440` RGBA transparent cutout, closer to portrait scale, still a single flattened bitmap at 351 px width.

### What `.livliv` v4 rigging requires (`FORMAT.md`)

1. **Separated transparent layer textures + inpainting:** Eyes (`eye_L`/`eye_R` + lossless eye-white masks, `lid_L`/`lid_R`, `iris_L`/`iris_R`, `shine_L`/`shine_R`), `brows`, `mouth` (separate `closed`, `half`, `open` frames + `visemes`), `face` skin **with eyes and mouth cleanly painted over underneath**, and `hair_front` / `hair_side` / `hair_back` / `body`.
2. **Higher resolution & clear front/3-quarter framing:** Standard `.livliv` portraits run ~1024–2048 px tall so 2.5D mesh warps, blinking eyelid curves, and mouth cutouts don't pixelate. (`FORMAT.md` §5 explicitly notes eyelid lash-curve deformation is tuned for front-facing faces without obstructed profiles.)
3. **Pixel anchors (`model.json`):** `head` ellipse, `face` rect, `neck_y`, `hair_root_y`, `eyes`, `brows`, `brow_lines`, `jaw` drop offsets, `chest`.

### Practical paths for Nyx art

| Option | Art effort | Compatibility with our Nyx assets | Quality & flexibility |
|---|---|---|---|
| **Option A — 1–2 Hero `.livliv` v4 rigs (only for `NyxDock` while idle/talking + `NyxDesk`)** | High once (upscale + cut/inpaint layers + generate mouth/eye frames for 1–2 canonical poses, or commission layered PSD -> `.livliv` v4) | Keeps Nyx's canonical look for her hero talking persona; leaves all 144 mood/outfit cards as `<NyxCard>` clips. | True 2.5D head turn, pointer follow, blinking, vowel-shaped lip-sync, zero recurring per-mood rigging cost. |
| **Option B — `PngBody` (`closed`/`open`/`blink`) or `companion.rs` `MouthPatches`** | Low–Medium (upscale `companion.png` + hero `hanging-out` crop, edit just the mouth/eyes for `closed`/`half`/`open` & `blink` patches) | Reuses Nyx's exact raster pixels (`Patch` anchors already modeled in `companion.rs`). | Clean crisp lip-sync & blinking + subtle breathing/bob, zero WebGL2 risk; no 2.5D head parallax. |
| **Option C — Rig all 144 clips** | Extreme / brittle | Baked BGs, 448×350 resolution, occluded faces (typing, turning away, headset mics) break automatic layer cutting. | Worse likeness fidelity than our existing video clips. |

---

## 5. Recommendation

### Verdict: **Conditional Yes — run a tightly scoped 2–3 day spike before choosing LivLiv vs `MouthPatches`**

**Keep `<NyxCard>` (`assets-nyx-anim-1` WebM/MP4 clips) for mood, outfit, and personality cards.** Do **not** replace them with runtime WebGL puppets.

Instead, run a **head-to-head prototype spike** on **one hero talking surface** (`NyxDock` during `VoiceSession` `speaking` state, or `NyxDesk`) comparing two candidates:

1. **Candidate 1 (`@livliv-dev/player` + `/voice`):**
   - Author **one** test `.livliv` v4 portrait bundle for Nyx's hero `hanging-out` / `companion.png` pose (transparent PNG/WebP layers, no LivLiv watermark) and wire `createSpeech(player)` via `new Response(bytes, { headers: { "content-type": spoken.mime } })` so zero CSP changes are needed.
   - Verify on **Windows WebView2**, **Linux WebKitGTK**, and **Android WebView**: WebGL2 startup (`Player.isSupported()`), alpha transparency in `NyxDesk`, audio-to-mouth sync + hardware mic AEC in `VoiceMode`, and GPU/VRAM usage with `maxPixelRatio: 1`.
2. **Candidate 2 (Zero-WebGL control baseline — `companion.rs` `MouthPatches` + 100 fps bandpass envelope):**
   - Port/write the tiny ~150-line Web Audio speech-band RMS detector (`150–1500 Hz`, 30 ms lead) to pick `closed` / `half` / `open` from `MouthPatches` + periodic blink from `eyes` (`Outfit` in `crates/grokhub-core/src/companion.rs`) overlaid on 1–2 upscaled hero poses.
   - Compare perceived liveliness against Candidate 1.

**Decision gate after the spike:**
- Ship **LivLiv `Player`** only if WebGL2 + WebView2/WebKitGTK compositing passes cleanly with no context-loss flicker and the layered Nyx `.livliv` rig preserves her likeness without uncanny layer seams.
- Otherwise ship **Candidate 2 (`MouthPatches` / `PngBody`)** for zero WebGL overhead and exact pixel likeness, reusing the exact `VoiceMode` `Response`-backed Web Audio scheduler pattern.

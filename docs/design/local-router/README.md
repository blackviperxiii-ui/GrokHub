# Local model router: design note

The desktop app picks and loads the right local model for each request:
chat, pictures, speech-to-text, text-to-speech, vision, and the speed,
balance and performance tiers. Scripts do the loading, unloading and
memory checks. The model only decides at decision points, such as writing
a picture's prompt.

This note ships with the first step. It records why a local chat looks
stuck on its model today, the target, and the build order.

## What happens today

- **Chat models swap by task, within limits.** Each on-device request is
  classified (`route/local_task.rs` `kind`), and `local_swap::plan` may
  choose another downloaded chat model. Some settings turn swapping off:
  Keep loaded, `GROKHUB_LOCAL_MODEL`, and Dynamic off. A switch to a bigger
  model also waits until the chat has been quiet for 20 seconds
  (`PENDING_STEP_UP`).
- **The pool holds chat models only.** `local_swap::on_disk` lists models
  where `is_chat()` is true, and `TaskKind` has no picture, voice or
  vision kind. One llama-server runs at a time (`local_sidecar` holds one
  `Option<Sidecar>`), so there is no least-recently-used list.
- **A picture depends on the chat model.** On a picture ask,
  `local_image::ready_on_device` only adds the `image_generate` schema and a
  system line. The small chat model must choose to call the tool. If it
  answers in text instead, nothing switches. This is the reported bug.
- **A signed-in Grok Imagine wins.** `tools/media.rs` `on_device` sends a
  clean prompt to Grok Imagine whenever it is reachable and signed in, even
  when the chat runs on this computer.
- **Drawing ignores memory.** `local_image::make` starts `sd-cli` with no
  RAM or VRAM check while the chat model stays loaded.
- **The chat never shows the picture.** `UiEvent::Tool` drops the result's
  picture, so the chat shows only a path.
- **Drafts can fall away.** The swap fit check (`local_scale::fits`)
  counts the parent only. A parent can then load without its paired draft
  (`local_draft.rs` decisions, and the autoconfig arm that drops it).
- **Voice is chosen by key.** Speech-to-text always goes to xAI, and
  on-device transcription has no caller. Text-to-speech uses xAI whenever a
  key is saved, and Kokoro otherwise.
- **The pick shows only in Settings.** The active model and its reason
  appear under Settings → Local AI → Running now. The chat shows neither,
  and the only pins are global (the Settings pick, Keep loaded, and
  Dynamic off).

## Target

1. **Needs.** A request has one need: chat (with a tier), picture,
   speech-to-text, text-to-speech, or vision. Scripts find the need from
   the request: a picture ask, an attached image, a voice turn, a "quick"
   or "take your time" ask, or the chat's pin. The model doesn't guess it.
2. **Deterministic dispatch.** A picture ask in a chat that runs on this
   computer always draws, whether or not the chat model would have called
   the tool. The chat model gets one constrained step,
   `tool_choice: "required"` with only `image_generate` offered, so it
   still writes the prompt. The hard-ban check and the picture guard read
   that prompt.
3. **On this computer stays on this computer.** A picture asked for in a
   chat that runs on device is drawn on device when a picture model is
   installed. Grok Imagine keeps cloud chats, the Imagine page, and
   computers without a picture model.
4. **Residency.** A ledger lists what is loaded: the chat server with its
   draft, the picture engine and the voice engines. It records each one's
   memory and when it was last used. Before anything loads, the router
   checks free RAM and VRAM (`local_scale::room`). If there isn't room, it
   unloads the least recently used entry. If there still isn't room, it
   declines with a plain line instead of overloading the computer.
5. **Drafts are part of their parent.** A paired parent and draft count
   as one entry, and their memory is added together. They always load and
   unload together. If the pair doesn't fit, the router picks a smaller
   tier's pair instead of dropping the draft. A model with no draft
   paired loads on its own, as it does today.
6. **Show and pin.** The chat shows which model answered and why, in one
   short line. A chat can pin a model or a tier, and the pin wins over
   swapping for that chat only.

## Build order

Each step is its own PR into `beta`, green on Linux and Windows.

1. **Pictures on device.** Force the picture step on a local chat. Draw on
   device when the step ran here. Check room before drawing: unload the
   chat server if that makes room, or decline. This note ships in the same
   PR.
2. **The picture in the chat.** `UiEvent::Tool` carries the made file.
   The tool card shows the picture through a command that reads only the
   app's media folders. This works for Grok Imagine results too.
3. **Residency ledger.** One ledger for the chat server, picture engine
   and voice engines, with memory sizes and LRU unloading. The picture
   path and Kokoro go through it. Shipped as `route/local_resident.rs`:
   - The chat server is read from the sidecar slot each time, so every
     way it unloads stays right. Short jobs (a picture, the picture guard,
     one spoken piece) hold a `Hold` while they run.
   - The sampler's reading can be up to 30 seconds old. A job that started
     after it is taken off the room, and a job it counted that has since
     finished is given back.
   - The picture hands its `Hold` to the guard, so keep-warm can't load
     the chat model in between.
   - A picture or the guard may unload the chat server. The voice never
     does, because that would reload the chat model on every turn. It
     speaks one piece at a time, and pieces of a reply the person talked
     over are dropped.
4. **Strict draft pairing.** Count the draft in every fit check. If the
   pair doesn't fit, step down a tier. Shipped in `route/local_spec.rs`
   and the files around it:
   - A model is paired when its drafter is on disk, the engine can run
     it, the tokenizer matches and no speed test or live check turned it
     off. A paired model always loads with its drafter.
   - The launch saves room for the drafter first. The context shrinks
     before the tier does, on the GPU and in the gaming setup's RAM.
   - The launch decides. If the pair still doesn't fit, the launch
     refuses it, and the next smaller model on disk is tried.
   - The swap planner only filters, and errs on the side of trying. It
     counts the drafter for a model the launch would pair
     (`local_autoconfig::pair_view`). It sizes the pair at its catalog
     size against all free VRAM plus a share of free RAM. That is more
     room than the launch's GPU budget, so the launch makes the close
     calls.
   - A model the launch refused is held: the model that started in its
     place stays. A pair is held until the model unloads, the governor
     state changes, or the memory the pair needed has 2 GB more free than
     at the refusal. That memory is VRAM when the launch planned the pair
     on a GPU, else RAM. A GPU pair with no VRAM reading has nothing to
     compare, so it is held until the model unloads or the state changes. The refusal's reading is the
     launch's own, taken after the previous model stopped. The reading
     now adds back what the model in its place uses. A start that ran out
     of memory is held until the next unload.
   - The drafter is never dropped to make room: not under Throttle, not
     after running out of memory, not on the CPU fallback, and not by
     auto-tune. A model split across GPU and RAM can't run a drafter, so
     it steps down too. An old out-of-memory layer cap doesn't refuse a
     pair. When the Settings GPU layers setting splits the model, the
     error says so.
   - While the speed test is pending, the pair loads together. When a
     start refused the pick, the speed test times the model that started
     in its place, even after it unloaded, and the refusal check waits.
     That record clears when the pick loads. A test plans as a fresh
     start would: the memory of the model that runs counts as free.
   - A start that comes before the hardware read finishes reads the
     hardware itself, so the first launch loads the drafter too.
   - Not yet: the warm-relaunch and residency sizes leave the drafter
     out, and the error names a smaller model by the model alone. Only
     the desktop app reads the hardware. A host that never starts that
     read runs the one-shot defaults, so a paired model loads alone there.
5. **Tiers, pin and the active-model line.** Speed, balance and
   performance come from the ask or the chat's pin. The active model
   and its reason show in the chat. Shipped in `route/local_scale.rs`
   (`chat_tier`), `route/local_swap.rs`, `route/local_pin.rs` and
   `route/local_live.rs` (`chat_line`):
   - The tier comes from the person's words in this turn, else the chat's
     setting in its tab menu ("Model for this chat": Auto, Speed, Balance,
     Performance, kept in `{config}/local-chat-pins.json`), else the
     automatic pick. The phrases are the cloud effort steer's
     (`ladder::Steer`): "think hard" asks for Performance, "be quick" for
     Speed. "Keep it short" alone asks for a short reply, not a faster
     model. A helper's turn and a side answer (`<chat>~side-<n>`, whose
     prompt quotes the main reply) read no words, **Tell the main task**
     text counts only its question, and the quick class is never a speed
     ask. Scripts decide; the model never picks its tier. A pin this build
     can't read (a newer tier, a hand edit) reads as Auto and is kept on
     the next write; a pins file that isn't JSON is never overwritten.
   - Speed runs the fastest model at or below the pick that fits with its
     drafter (the lighter one on a tie). Performance runs the biggest model
     above the pick that fits with its drafter and headroom at 12 tok/s or
     more, on battery or a busy CPU too. Balance runs the pick, with no
     model picked for the task and no lighter one while saving power. When
     no pair fits, the walk steps down a tier as before; the drafter is
     never dropped.
   - An ask or a setting loads now, past the 120 s cooldown and the wait
     for a quiet chat. A reply in flight, Keep loaded, a forced model file,
     a low battery and a game still win. On a low battery Performance loads
     nothing bigger, even when no smaller model is on disk.
   - The line under each on-device reply names the model, whether its
     draft ran, and why: the pick, the tier the chat asked for, why an ask
     didn't get its tier (no faster or bigger pair fits, another chat was
     answering, a low battery, Keep loaded or a forced file), or the
     stand-in's reason, the same clause (`StandIn::reason`) the Settings row
     uses. An ask got its tier when the model running is the one its plan
     aimed at, whatever label that model loaded under. A setting refused
     because another chat was answering loads with the next reply; words
     asked only for this one. It isn't saved, so a reopened chat shows none.
     The Settings row keeps the running model's own label when an ask is
     met by the model already running.
   - Not yet: a pin for one model (the store keeps room for it), the line
     for cloud turns, and a tier that the cloud router reads. Two chats set
     to different tiers reload the model on each turn. The speed estimate doesn't count the drafter's speedup.
6. **Voice on device.** On-device speech-to-text, then text-to-speech
   chosen by need (an on-device chat or Offline only) instead of by
   whether a key is saved. #2032 built the shared voice router
   (`route/voice_route.rs`: `Auto`, `Local only`, `Pinned`; `plan_stage`
   over a `VoiceEnv`), with Offline only, Grok sign-in, xAI cooldowns and
   automatic downloads. This step adds the chat's own need to it:
   - The router writes where each chat's newest step runs
     (`route/local_live.rs`: `note_step_here`, `runs_here`,
     `must_stay_here`) before the model call, again when xAI or a provider
     fails over to this computer, and when the step ends. The app's
     background calls and helpers don't write it. A chat with no step since
     launch reads as online, unless the voice every chat speaks in is 18+:
     that comes from the saved settings, so it holds from launch.
   - `VoiceEnv` gains `chat_here` and `chat_must_stay` (an 18+ voice). In
     `Auto`, a chat that runs here hears and speaks only on this computer
     once the on-device voice is ready, with no xAI behind it; until then
     xAI stands in, with a line, while the on-device voice downloads
     (`wants_download`). A bare whisper on PATH names no model, so it
     doesn't count as ready for a chat that runs here. An 18+ chat never
     plans xAI, a pin to Grok Voice 3 included: it speaks here when the
     voice is downloaded, else downloads it or refuses with the real reason
     (Local AI off, or not downloaded).
   - Every voice call carries the chat: `transcribe`, `voice_live_start`,
     `voice_live_hear`, `voice_speak`, and `voice_live_chat` when voice
     mode follows another chat. A move to a chat that may not stream drops
     the open stream (and its caption) at once, before any keychain read,
     and one still opening for the chat before, or for an earlier voice
     session, is never attached. A reply's tail is spoken by its own
     chat's need.
   - Hearing on device reserves `Kind::Hearing` with `may_unload = false`,
     one clip at a time (whisper too, held at 2 GB): the voice never unloads or steps down the chat
     pair or its draft, and declines with a line when there's no room. The
     clip goes to a fresh temp file only this user can read
     (`grokhub-hear-…`, mode 0600), removed on every path; both pipes are
     read on threads and drained past the cap; the time limit is 20 s plus
     twice the clip's length.
   - The line names the engine and why ("Spoken on this computer with
     Kokoro text to speech: this chat runs here."; `stage_line`). It shows
     for the on-device engine, an xAI fallback, or xAI standing in for a
     chat that runs here, never for a plain cloud chat. Voice mode shows it
     on its own line over the composer; push to talk shows it there as a
     hint, not on the error line.
   - Not yet: the process-wide `local_warm` still puts on-device speech
     first for a cloud chat while a local model is loaded (an existing
     test locks it in); a `Pinned` Grok Voice 3 wins over a chat that runs
     here (only 18+ overrides it); whisper on PATH runs with no `-m`
     model; download notices still show on the error line; the record of
     where a chat runs is in memory only, so in a chat that isn't 18+ the
     first spoken turn after a restart, before its first step, follows the
     cloud route; a voice that dynamic chat hands an 18+ turn to is known
     only once that step ran.
7. **Vision.** Load the vision projector (`--mmproj`) with a model that
   has one when a picture is attached to an on-device chat. Only the first
   part is built (`route/local_vision.rs`): a picture no longer breaks a
   chat that runs here. On-device vision (loading a projector, counting it
   in the residency ledger and the fit checks with the parent and its
   drafter, stepping down a tier when they can't fit) is not built yet, so
   this step stays open.
   - Before this step a picture in a chat that ran here (offline, 18+, or
     the no-keys and cloud-down fallbacks) failed: the local tier lists no
     `input_modalities`, so `policy::fits` let it take the picture, the
     chat server had no `--mmproj`, and `llama-server` refused the
     `image_url` part. The picture stayed in history, so every later local
     turn of that chat failed the same way.
   - On device: no launch carries a projector yet, so `serve_request` turns
     every picture into a short text note (`BLIND_NOTE`). When the
     person's newest message carried a picture, the line under the reply
     adds "The picture wasn't read: no model on this computer can see
     pictures yet." (alone, when the reply names no model). An older
     picture the chat carries adds nothing.
   - Routing: online, a chat is never routed to the device (`LOCAL_CLASSES`
     holds background steps only), so a chat picture already goes to an
     online model by the image fit. For a step the device would take (a
     warm background step), with a picture in the newest message, when the
     step may leave this computer (not 18+, voice not Local only, not
     sensitive without your grant, not offline), `Router::choose` picks
     again among the online models listed as reading pictures
     (`input_modalities` holds `image`) and pushes `local:no_vision`. A
     model with unknown input kinds doesn't count. With none left, the
     device keeps the step, without the picture. A step that went online
     anyway gets no `local:no_vision`.
   - Left for the next part, in order:
     1. Real-engine check on b12017: does a draft run beside `--mmproj`?
        Older builds refused speculative decoding with a projector. If it
        refuses, strict pairing means no on-device vision for any paired
        Gemma row.
     2. Launch: a projector field on `LaunchArgs` that adds `--mmproj
        <file>`, plus `--no-mmproj-offload` whenever `ngl == 0` or
        `cpu_only`. `--device none` does not keep the projector off the
        GPU: llama.cpp's `clip_ctx` starts the GPU backend whenever
        `mmproj_use_gpu` is set and never reads the `--device` list. Both
        flags are in the pinned b12017 `--help`.
     3. Catalog: five `vision-projector` rows (`remote_file =
        "mmproj-model-bf16.gguf"`, a unique `file` each) with `projector =`
        on the Gemma 4 chat rows, version 5. Sizes (bytes) from the Hub:
        E2B 986,833,408; E4B 991,552,000; 12B 175,115,776; 26B-A4B
        1,194,827,872; 31B 1,200,726,112. The SHA-256 values still need
        fetching; the Hub listing reached from this box shows sizes only.
     4. Sizing: `vision_mb` (pair plus projector), the projector counted in
        `planned_as`, `chat_entry`/`chat_mem` and `load_bytes`;
        `warm_fits` refuses a projector; a plan sized with the projector
        never writes `LAST_REFUSAL`.
     5. Swap: `vision_plan` beside `plan()` (keep-loaded, forced file,
        held refusals and gaming rules apply), `StandIn::Vision` ("Your
        pick comes back once the chat is quiet."), the no-projector retry
        ahead of the drop-draft and out-of-memory branches, and a lasting
        record when the engine refuses a draft with a projector.
     6. A "can see pictures" input on `LocalGate`, from the same inputs the
        swap builds, so routing and loading agree; `serve_request` then
        keeps pictures for a launch with its projector.
   - Open: a screenshot or `read_file` image the model took itself counts
     as the person's newest message (`last_user_text`). Keeping those on
     device is a product call.

## Done when

On Linux, one chat that runs on this computer asks for a picture. It
switches to the picture model, shows the picture in the same chat, then
answers the next message on the chat model. A voice request uses the voice
model. A speed or performance ask loads that tier with its draft. When
memory is low, the router unloads or declines instead of overloading the
computer.

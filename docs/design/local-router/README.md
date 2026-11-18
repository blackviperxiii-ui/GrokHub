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
   and its reason show in the chat.
6. **Voice on device.** On-device speech-to-text, then text-to-speech
   chosen by need (an on-device chat or Offline only) instead of by
   whether a key is saved.
7. **Vision.** Load the vision projector (`--mmproj`) with a model that
   has one when a picture is attached to an on-device chat.

## Done when

On Linux, one chat that runs on this computer asks for a picture. It
switches to the picture model, shows the picture in the same chat, then
answers the next message on the chat model. A voice request uses the voice
model. A speed or performance ask loads that tier with its draft. When
memory is low, the router unloads or declines instead of overloading the
computer.

# Empty-chat greeting blurb

**Date:** 2026-08-15  
**Version:** 2.0.0 (do not bump)  
**Override:** Native Rust only.

## Spec

New chats (empty, not Scratch) show one faint italic line under the GrokHub wordmark and above the composer.

- Local rank is instant. The line is a situation pick, not a memory dump: time of day, first name, signed-in, first-run vs returning, last project title.
- First-run unsigned: `Morning. Connect Grok to start.` First-run signed: `Morning, Jeremy. The cabin is ready.` Returning with a titled thread: `Evening, Jeremy. Night cabin is still open.` Returning unsigned: `Evening. Sign in to pick up.` Otherwise just time + name.
- Do not quote `USER.md` / `MEMORY.md` / fail receipts on the painted line. Fast mode may still read them. Debounce 800ms. Never blocks send. No spinner.
- Paint uses the whisper token (dimmer than tertiary chrome), 13px italics. Seen, not a second headline.
- Cap 92 characters. Secrets never enter the prompt payload or the painted line.
- Scratch stays blank. The line vanishes on the first message.

Do not clone grok.com’s large sit-down greeting. Do not inject the blurb as a chat bubble.

# Empty-chat greeting blurb

**Date:** 2026-08-15  
**Version:** 2.0.0 (do not bump)  
**Override:** Native Rust only.

## Spec

New chats (empty, not Scratch) show one faint italic line under the GrokHub wordmark and above the composer.

- Local rank is instant from `USER.md`, `MEMORY.md`, learned insights, and OAuth display name.
- Fast mode (`grok-3-mini-fast`) rewrites the line when the memory fingerprint changes. Debounce 800ms. Never blocks send. No spinner.
- Paint uses the whisper token (dimmer than tertiary chrome), 13px italics. Seen, not a second headline.
- Cap 92 characters. Secrets never enter the prompt payload or the painted line.
- Scratch stays blank. The line vanishes on the first message.

Do not clone grok.com’s large sit-down greeting. Do not inject the blurb as a chat bubble.

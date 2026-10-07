# Compass: grokhub-ffi (C ABI)

## Owns
- `libgrokhub_ffi`, a `cdylib` + `rlib` that exposes a few grokhub-core answers to Android and Windows callers so they do not grow a second protocol: hub kind, default port, pair code make/normalize, Imagine and voice model picks, `forbidden_reason` as an int, and `slash_kind`.
- The hand-written header `crates/grokhub-ffi/include/grokhub.h`.
## Quick commands
- `cargo test -p grokhub-ffi`
- `cargo build -p grokhub-ffi` (Linux CI builds it with the app and hub; Windows CI does not)
## Key files
- `crates/grokhub-ffi/src/lib.rs`: every `#[no_mangle] extern "C"` fn, plus `cstr` / `read` helpers and the one ABI test.
- `crates/grokhub-ffi/include/grokhub.h`: the C declarations callers compile against.
- `crates/grokhub-ffi/Cargo.toml`: crate type and its only dependency, grokhub-core.
## Change recipe
- Add the `#[no_mangle] pub extern "C" fn grokhub_<name>` in `lib.rs`, call a grokhub-core fn (no logic here), add the matching prototype to `grokhub.h`, and extend `pair_and_limits_c_abi` with a literal expected value.
- Any fn that returns `*mut c_char` must be freed by the caller with `grokhub_string_free`; say so in the header if it is new.
## What breaks it
- Renaming or changing a signature: the header is not generated (no cbindgen), so C callers break silently. Edit both files together.
- Changing core answers it forwards: `DEFAULT_PORT` (18766), `HUB_KIND`, pair code shape (7 chars with a dash, `CODE_ALPH`), `dedicated_imagine_model`, `dedicated_voice_model`, or a `slash_kind` string.
## What depends on it
- Android and Windows native callers (`docs/REFERENCE.md`: link `libgrokhub_ffi`, include the header). Nothing in this workspace links it.
## Non-obvious
- Null input: `grokhub_normalize_code` and `grokhub_slash_kind` return null; the model fns treat null as "no user pick"; `grokhub_forbidden` returns 0.
- `cstr` maps a string with an interior NUL to an empty string instead of panicking across the ABI.
- `read` uses `to_string_lossy`, so invalid UTF-8 is replaced, not rejected.
- `grokhub_string_free` is the only `unsafe` fn; its doc says the pointer must come from a `grokhub_*` fn.
- Keep this crate logic-free: the crate doc says "same grokhub-core", so fix behavior in core.
## See also
- [grokhub-core](grokhub-core.md), [grokhub-hub](grokhub-hub.md), [slash](slash.md)

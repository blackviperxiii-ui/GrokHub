# Native Rust cabin — implementation

## Done

- [x] Workspace `crates/grokhub-core` + `crates/grokhub-hub` + `crates/grokhub-app`
- [x] Core + hub HTTP + cabin host/config tests
- [x] Native `grokhub` cabin (egui)
- [x] Hub as a library + `grokhub-hub` CLI
- [x] AUR / install / systemd / release point at cargo binaries
- [x] Electron, Vite, `src/`, `desktop/`, npm, Playwright removed
- [x] CI: `cargo test --workspace`, `cargo build`, no-Electron tree gate
- [x] Slash `/approve` `/memory` `/recall` `/forget`
- [x] Host rails + secret redact; hub/chat persist (no lastFrame)
- [x] `grokhub --hub` / `--doctor` / `--version`
- [x] Workboard, SKILL.md, Imagine, windshield, Hey Grok
- [x] `grokhub-ffi` C ABI for Android / Windows

## Next (sibling / later)

- [ ] UniFFI bindings on `grokhub-core` for Grok-Hub-Android
- [ ] Windows build of the same `grokhub` binary

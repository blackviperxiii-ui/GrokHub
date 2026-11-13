//! The Code tab's logic, shared by the cabin and the Tauri shell: the
//! read-only repo view, test runs, the push guard and PRs, the checks before
//! a commit, keychain secrets, deploys, editor file I/O and the terminal. No
//! UI framework here; each app wraps these calls in its own commands or
//! widgets.

#![cfg_attr(not(test), warn(clippy::unwrap_used, clippy::expect_used))]

pub mod activity;
pub mod chat_push;
pub mod commit;
pub mod deploy;
pub mod files;
pub mod repo;
pub mod rollback;
pub mod runs;
pub mod secrets;
pub mod ship;
pub mod term;

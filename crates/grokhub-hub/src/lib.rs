//! In-process LAN hub. The native app embeds this. `grokhub-hub` CLI is a thin main.

mod server;

pub use server::{serve, serve_background, serve_lan};

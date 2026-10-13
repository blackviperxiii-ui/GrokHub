//! User-only DACL for Windows secret files. Owned by `grokhub-core::win_acl`
//! so hub-state.json and secrets.json share one rail.

#![cfg(windows)]

pub(crate) use grokhub_core::win_acl::*;

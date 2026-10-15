//! Minimal, flowing Workboard cards (Flo agent-inbox style). `board_ui.rs`
//! keeps the boards, columns, and open-card chat; this folder owns the card
//! look as slices land: tokens (S1), the compact row (S2), then expand and
//! step timeline, inline actions, drag shrink, empty state, and the
//! board/list switch.

mod compact;
#[cfg(test)]
mod tests;
mod tokens;

pub(super) use compact::{paint_compact_card, CompactRow};

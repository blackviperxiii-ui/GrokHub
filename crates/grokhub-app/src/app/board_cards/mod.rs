//! Minimal, flowing Workboard cards (Flo agent-inbox style). `board_ui.rs`
//! keeps the boards, columns, and open-card chat; this folder will own the card
//! look as slices land: tokens (S1), compact row, expand and step timeline,
//! inline actions, drag shrink, empty state, and the board/list switch.
//!
//! Tokens are test-only until the compact card (S2) paints with them, so the
//! binary carries no dead code in between.

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tokens;

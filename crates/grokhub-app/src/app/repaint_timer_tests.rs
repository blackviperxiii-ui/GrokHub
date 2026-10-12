//! PR2: short `request_repaint_after` timers stay behind busy / animating
//! state so Home and Settings idle do not wake the frame every 16–150 ms.

fn body<'a>(src: &'a str, name: &str) -> &'a str {
    let needle = format!("fn {name}(");
    let start = src
        .find(&needle)
        .unwrap_or_else(|| panic!("missing {name}"));
    let after = &src[start..];
    let end = after[needle.len()..]
        .find("\n    fn ")
        .or_else(|| after[needle.len()..].find("\nfn "))
        .map(|i| needle.len() + i)
        .unwrap_or(after.len());
    &after[..end]
}

#[test]
fn palette_repaint_only_while_search_inflight() {
    let src = include_str!("palette.rs").replace("\r\n", "\n");
    let tick = body(&src, "tick_palette_search");
    assert!(
        tick.contains("request_repaint_after"),
        "palette still polls while a walk is open: {tick}"
    );
    assert!(
        tick.contains("palette_file_rx.is_some()") && tick.contains("palette_step_rx.is_some()"),
        "short tick must require an in-flight file or step search: {tick}"
    );
    let after_kick = tick
        .split("kick_palette_search()")
        .nth(1)
        .expect("kick then maybe tick");
    assert!(
        !after_kick
            .trim_start()
            .starts_with("ctx.request_repaint_after")
            && after_kick.contains("palette_file_rx.is_some()"),
        "idle after a saved search must not schedule 60 ms: {tick}"
    );
}

#[test]
fn pulse_tab_repaint_only_while_crossfade_moves() {
    let src = include_str!("pulse_ui.rs").replace("\r\n", "\n");
    let pulse = body(&src, "ui_pulse");
    assert!(
        pulse.contains("motion_ok")
            && pulse.contains("0.001 < tab_t")
            && pulse.contains("tab_t < 0.999"),
        "16 ms crossfade tick only while motion is on and t is mid-way: {pulse}"
    );
    let silence = body(&src, "paint_pulse_silence");
    let loading_start = silence
        .find("if !feed && self.ideas_rx.is_some()")
        .expect("ideas loading arm");
    let loading = &silence[loading_start..];
    let loading_end = loading.find("\n        if feed {").unwrap_or(loading.len());
    let loading = &loading[..loading_end];
    assert!(
        loading.contains("request_repaint_after"),
        "250 ms silence tick stays inside the ideas-loading arm: {loading}"
    );
}

#[test]
fn privacy_repaint_only_while_keyring_pending() {
    let src = include_str!("privacy_ui.rs").replace("\r\n", "\n");
    let rows = body(&src, "ui_privacy_rows");
    let gate = rows
        .find("if self.harness.lock_pending {")
        .and_then(|i| rows[i..].split("\n        if let Some(why)").next())
        .expect("lock_pending gate before request_repaint_after");
    assert!(
        gate.contains("request_repaint_after") && !gate.contains("ledger.pending()"),
        "150 ms tick only while lock_pending, not ledger.pending alone: {gate}"
    );
}

#[test]
fn scope_folder_pick_repaint_only_while_dialog_open() {
    let src = include_str!("scope_ui.rs").replace("\r\n", "\n");
    let poll = body(&src, "poll_folder_pick");
    assert!(
        poll.contains("TryRecvError::Empty") && poll.contains("request_repaint_after"),
        "150 ms tick only while the folder dialog has not answered: {poll}"
    );
    assert!(
        poll.contains("scope_pick_rx.take()")
            && poll
                .split("request_repaint_after")
                .next()
                .unwrap()
                .contains("Empty"),
        "no rx means no short timer: {poll}"
    );
}

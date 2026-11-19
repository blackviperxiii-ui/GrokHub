use super::*;
use grokhub_core::local_governor::Level;
use grokhub_core::local_setup;

fn m(id: &str) -> &'static LocalModel {
    local_setup::model_by_id(id).expect("catalog model")
}

#[test]
fn the_task_class_and_size_set_the_need() {
    assert_eq!(need("debug", 50, "fix it"), Need::Hard);
    assert_eq!(need("code:multi-file", 50, "x"), Need::Hard);
    assert_eq!(need("chat:quick", 50, "hi"), Need::Light);
    assert_eq!(need("desktop:soft", 50, "click"), Need::Light);
    assert_eq!(need("background:summarize", 900, "text"), Need::Background);
    assert_eq!(need("chat:turn", 5, "what's up"), Need::Normal);
    assert_eq!(need("chat:default", HARD_TOKENS + 1, "x"), Need::Hard);
}

#[test]
fn room_fit_and_speed_follow_free_memory() {
    let room = Room {
        free_vram_mb: Some(16_000),
        free_ram_mb: 20_000,
        cpu_busy_pct: Some(10),
        on_battery: false,
        unified: false,
    };
    assert_eq!(usable_mb(&room, 0), 32_000);
    assert_eq!(usable_mb(&room, 5_000), 37_000);
    let moe = m("gemma-4-26b-a4b-it-abliterated-q4_k");
    let dense = m("gemma-4-31b-it-abliterated-q4_k");
    let mid = m("gemma-4-12b-it-abliterated-q4_k");
    assert!(fits(moe.run_mb, &room, 0, HEADROOM_MB));
    assert_eq!(est_tok_s(mid, mid.run_mb, &room, 0), 35, "all on the GPU");
    assert_eq!(est_tok_s(moe, moe.run_mb, &room, 0), 40, "experts in RAM");
    assert_eq!(
        est_tok_s(dense, dense.run_mb, &room, 0),
        15,
        "a dense split"
    );
    let cpu = Room {
        free_vram_mb: None,
        ..room
    };
    assert_eq!(est_tok_s(mid, mid.run_mb, &cpu, 0), 3);
    let mac = Room {
        unified: true,
        ..cpu
    };
    assert_eq!(est_tok_s(mid, mid.run_mb, &mac, 0), 20);
    assert!(performance_ready(&room));
    assert!(!performance_ready(&Room {
        cpu_busy_pct: Some(CPU_FREE_PCT + 1),
        ..room
    }));
    assert!(!performance_ready(&Room {
        on_battery: true,
        ..room
    }));
}

#[test]
fn a_pair_fits_only_when_the_model_and_its_drafter_both_fit() {
    // 9 GB free RAM, no GPU reading: 7,200 MB usable.
    let room = Room {
        free_vram_mb: None,
        free_ram_mb: 9_000,
        cpu_busy_pct: Some(10),
        on_battery: false,
        unified: false,
    };
    assert_eq!(usable_mb(&room, 0), 7_200);
    // Gemma 4 E4B with its heads (6,288 MB) fits; the 12B pair (9,477) doesn't.
    assert!(fits(6_288, &room, 0, 0));
    assert!(!fits(9_477, &room, 0, 0));
    // The drafter counts against the GPU: the 12B pair no longer runs all on it.
    let gpu = Room {
        free_vram_mb: Some(9_000),
        ..room
    };
    let mid = m("gemma-4-12b-it-abliterated-q4_k");
    assert_eq!(est_tok_s(mid, 8_399, &gpu, 0), 35);
    assert_eq!(est_tok_s(mid, 9_477, &gpu, 0), 18);
}

#[test]
fn a_reading_goes_stale_and_the_governor_reason_sets_gaming_or_saving() {
    clear_room();
    let r = Room::default();
    set_room(r, 1_000);
    assert_eq!(room(1_000 + ROOM_STALE_MS), Some(r));
    assert_eq!(room(1_001 + ROOM_STALE_MS), None);
    clear_room();
    let g = scale_for(Level::Throttle, "a game is running", None);
    assert!(g.gaming && !g.saving);
    let b = scale_for(Level::Throttle, "another app is keeping the GPU busy", None);
    assert!(b.gaming);
    let s = scale_for(Level::Throttle, "on battery", None);
    assert!(!s.gaming && s.saving);
    let n = scale_for(Level::Normal, "normal use", None);
    assert!(!n.gaming && !n.saving);
}

#[test]
fn the_need_guard_sets_and_restores_the_threads_need() {
    assert_eq!(current_need(), Need::Normal);
    {
        let _g = NeedGuard::set(Need::Hard);
        assert_eq!(current_need(), Need::Hard);
    }
    assert_eq!(current_need(), Need::Normal);
}

#[test]
fn only_from_now_on_words_set_a_standing_tier_and_back_to_auto_drops_it() {
    use SpeedQuality::{Quality, Speed};
    assert_eq!(
        standing_ask("From now on, be quick.", false, false),
        Some(Some(Speed))
    );
    assert_eq!(
        standing_ask("Think hard for the rest of this chat", false, false),
        Some(Some(Quality))
    );
    assert_eq!(
        standing_ask("OK, back to auto please", false, false),
        Some(None)
    );
    // One turn's ask stays one turn's ask.
    assert_eq!(standing_ask("be quick", false, false), None);
    // A short reply isn't a faster model, even from now on.
    assert_eq!(
        standing_ask("from now on keep it short", false, false),
        None
    );
    assert_eq!(
        standing_ask("from now on, call me user", false, false),
        None
    );
    // A helper's or side answer's text is never the person's ask.
    assert_eq!(standing_ask("from now on, be quick", false, true), None);
    // Care-first lets the Brief pick effort, so words set no tier.
    assert_eq!(standing_ask("from now on, be quick", true, false), None);
}

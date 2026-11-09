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
    assert!(fits(moe, &room, 0, HEADROOM_MB));
    assert_eq!(est_tok_s(mid, &room, 0), 35, "all on the GPU");
    assert_eq!(est_tok_s(moe, &room, 0), 40, "experts in RAM");
    assert_eq!(est_tok_s(dense, &room, 0), 15, "a dense split");
    let cpu = Room {
        free_vram_mb: None,
        ..room
    };
    assert_eq!(est_tok_s(mid, &cpu, 0), 3);
    let mac = Room {
        unified: true,
        ..cpu
    };
    assert_eq!(est_tok_s(mid, &mac, 0), 20);
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

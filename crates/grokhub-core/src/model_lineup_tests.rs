use super::*;
use crate::local_picker::BenchCache;
use crate::local_setup::CpuSimd;
use crate::model_library::rows;

fn pc(gpus: &[u64], ram_mb: u64, free_disk_mb: u64) -> Profile {
    Profile {
        gpus_vram_mb: gpus.to_vec(),
        unified: false,
        ram_mb,
        free_disk_mb,
        simd: CpuSimd::Avx2,
        llama_build: None,
    }
}

fn lineup_for(p: &Profile, on: &[&str], free: Option<u64>) -> Lineup {
    let rows = rows(p, &BenchCache::new(), |m| on.contains(&m.id));
    recommend(&rows, p, free)
}

fn slot(l: &Lineup, g: Goal) -> &Slot {
    l.slot(g).unwrap()
}

fn id(s: &Slot) -> Option<&'static str> {
    s.pick.map(|m| m.id)
}

const E2B: &str = "gemma-4-e2b-it-abliterated-q4_k";
const E4B: &str = "gemma-4-e4b-it-abliterated-q4_k";
const G26: &str = "gemma-4-26b-a4b-it-abliterated-q4_k";
const G31: &str = "gemma-4-31b-it-abliterated-q4_k";

#[test]
fn an_8gb_gpu_gets_a_quick_model_an_everyday_one_a_step_up_and_pictures() {
    let p = pc(&[8_192], 32_768, 500_000);
    let l = lineup_for(&p, &[], Some(500_000));
    assert_eq!(l.slots.len(), Goal::ALL.len());
    assert_eq!(
        id(slot(&l, Goal::Everyday)),
        Some(E4B),
        "the picker's default"
    );
    assert_eq!(
        Some(E4B),
        crate::local_picker::preselected(&p).map(|m| m.id),
        "Everyday is what auto setup picks"
    );
    assert_eq!(id(slot(&l, Goal::Speed)), Some(E2B));
    // A mixture-of-experts model spills to RAM and still clears the floor.
    assert_eq!(id(slot(&l, Goal::Performance)), Some(G26));
    assert!(est_tok_s(&p, slot(&l, Goal::Performance).pick.unwrap()) >= PERFORMANCE_FLOOR_TOK_S);
    assert_eq!(id(slot(&l, Goal::Image)), Some("z-image-turbo-q4_k"));
    assert_eq!(slot(&l, Goal::Image).extras[0].kind, ModelKind::ImageGuard);
    let video = slot(&l, Goal::Video);
    assert!(video.pick.is_none());
    assert_eq!(video.note, "No on-device video models yet.");
    // Hot-swap fit: Speed is smaller than Balance, Performance bigger.
    let run = |g| slot(&l, g).pick.unwrap().run_mb;
    assert!(run(Goal::Speed) < run(Goal::Everyday));
    assert!(run(Goal::Performance) > run(Goal::Everyday));
}

#[test]
fn the_queue_puts_the_everyday_model_first_and_each_draft_model_after_its_chat_model() {
    let p = pc(&[8_192], 32_768, 500_000);
    let l = lineup_for(&p, &[], Some(500_000));
    let ids: Vec<&str> = l.queue.iter().map(|m| m.id).collect();
    assert_eq!(ids[0], E4B);
    for (i, m) in l.queue.iter().enumerate() {
        if m.kind == ModelKind::Drafter {
            assert_eq!(Some(l.queue[i - 1].id), m.pairs_with, "{ids:?}");
        }
    }
    assert!(
        l.queue.iter().all(|m| !m.optional),
        "never an optional model"
    );
    let mut dedup = ids.clone();
    dedup.sort_unstable();
    dedup.dedup();
    assert_eq!(dedup.len(), ids.len(), "each model once");
    let line = lineup_line(&l);
    assert!(
        line.starts_with(&format!("{} models, ", l.queue.len())),
        "{line}"
    );
    for g in Goal::ALL {
        assert!(line.contains(&format!("{}: ", g.label())), "{line}");
    }
}

#[test]
fn a_big_gpu_runs_the_fast_moe_every_day_and_steps_up_to_the_31b() {
    let p = pc(&[24_576], 65_536, 500_000);
    let l = lineup_for(&p, &[], None);
    // Speed first: the 26B-A4B MoE (about 60 tok/s) is the everyday model,
    // and the dense 31B (about 35 tok/s) is what hard tasks step up to.
    assert_eq!(
        id(slot(&l, Goal::Everyday)),
        Some("gemma-4-26b-a4b-it-abliterated-q4_k")
    );
    assert_eq!(id(slot(&l, Goal::Performance)), Some(G31));
    let speed = slot(&l, Goal::Speed).pick.unwrap();
    assert!(speed.run_mb < slot(&l, Goal::Everyday).pick.unwrap().run_mb);
}

#[test]
fn a_cpu_only_pc_skips_a_step_up_too_slow_for_hot_swap() {
    let p = pc(&[], 16_384, 500_000);
    let l = lineup_for(&p, &[], None);
    assert_eq!(id(slot(&l, Goal::Everyday)), Some(E4B));
    assert_eq!(id(slot(&l, Goal::Speed)), Some(E2B));
    assert!(slot(&l, Goal::Performance).pick.is_none());
}

#[test]
fn what_is_on_disk_stays_in_the_lineup_but_out_of_the_queue() {
    let p = pc(&[8_192], 32_768, 500_000);
    let l = lineup_for(&p, &[E4B, "gemma-4-e4b-it-abliterated-mtp"], Some(500_000));
    let balance = slot(&l, Goal::Everyday);
    assert_eq!(id(balance), Some(E4B));
    assert!(balance.on_disk);
    assert!(l.queue.iter().all(|m| !m.id.starts_with("gemma-4-e4b")));
    assert!(slot_line(balance).ends_with("(downloaded)"));
}

#[test]
fn the_queue_stops_before_the_disk_runs_out_and_says_what_waits() {
    let p = pc(&[8_192], 32_768, 500_000);
    let full = lineup_for(&p, &[], Some(500_000));
    let need: u64 = full
        .queue
        .iter()
        .take_while(|m| m.id != G26)
        .map(|m| m.download_mb)
        .sum();
    let l = lineup_for(&p, &[], Some(need + DISK_HEADROOM_MB));
    let mb: u64 = l.queue.iter().map(|m| m.download_mb).sum();
    assert!(mb <= need, "{mb} > {need}");
    assert!(l.queue.iter().all(|m| m.id != G26));
    let perf = slot(&l, Goal::Performance);
    assert_eq!(id(perf), Some(G26));
    assert!(perf.note.starts_with("Needs "), "{}", perf.note);
    // A model and its draft model come together or not at all.
    assert!(!l.queue.iter().any(|m| m.pairs_with == Some(G26)));
}

#[test]
fn nothing_to_fetch_says_so_and_a_pc_without_avx_gets_no_chat_models() {
    let p = pc(&[8_192], 32_768, 500_000);
    let full = lineup_for(&p, &[], None);
    let all: Vec<&str> = full.queue.iter().map(|m| m.id).collect();
    let done = lineup_for(&p, &all, None);
    assert!(done.queue.is_empty());
    assert!(lineup_line(&done).starts_with("This PC has its best lineup."));
    let old = Profile {
        simd: CpuSimd::NoAvx,
        ..p
    };
    let l = lineup_for(&old, &[], None);
    for g in [Goal::Speed, Goal::Everyday, Goal::Performance] {
        assert!(slot(&l, g).pick.is_none());
        assert!(slot(&l, g).note.contains("AVX"), "{}", slot(&l, g).note);
    }
    assert!(l.queue.iter().all(|m| !m.is_chat()));
}

#[test]
fn a_gpu_starts_from_its_tiers_speed_estimate() {
    let e2b = ls::model_by_id(E2B).expect("catalog model");
    let speeds = e2b.profile.tok_s_tier;
    let (big, small) = (pc(&[24_564], 32_768, 0), pc(&[6_141], 16_384, 0));
    assert_eq!(Some(est_tok_s(&big, e2b)), speeds.t4);
    assert_eq!(Some(est_tok_s(&small, e2b)), speeds.t1);
    assert!(
        speeds.t4 > speeds.t1,
        "a bigger card reads the weights faster"
    );
}

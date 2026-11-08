use super::*;

fn m(id: &str) -> &'static LocalModel {
    local_setup::model_by_id(id).expect("catalog model")
}

fn file(id: &str) -> PathBuf {
    PathBuf::from("/models").join(m(id).file)
}

fn tmp(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "grokhub-swap-{label}-{}-{}",
        std::process::id(),
        super::super::local_sidecar::unix_ms()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const BIG: &str = "gemma-4-12b-it-abliterated-q4_k";
const MID: &str = "gemma-4-e4b-it-abliterated-q4_k";
const SMALL: &str = "gemma-4-e2b-it-abliterated-q4_k";

fn disk() -> Vec<(&'static LocalModel, PathBuf)> {
    [BIG, MID, SMALL]
        .iter()
        .map(|id| (m(id), file(id)))
        .collect()
}

fn now<'a>(home: &'a Path, on_disk: &'a [(&'static LocalModel, PathBuf)]) -> Now<'a> {
    Now {
        home,
        loaded: None,
        on_disk,
        low_battery: false,
        kept_loaded: false,
        forced: false,
        in_flight: 0,
        secs_since_swap: u64::MAX,
    }
}

#[test]
fn the_pick_runs_and_a_low_battery_swaps_to_the_smallest_model_and_back() {
    let disk = disk();
    let home = file(BIG);
    let n = now(&home, &disk);
    assert_eq!(
        plan(n),
        Plan::Load {
            model: home.clone(),
            stand_in: None
        },
        "nothing runs: the pick starts"
    );
    let running = Now {
        loaded: Some((&home, None)),
        ..n
    };
    assert_eq!(plan(running), Plan::Keep);

    let low = Now {
        low_battery: true,
        ..running
    };
    assert_eq!(
        plan(low),
        Plan::Load {
            model: file(SMALL),
            stand_in: Some(StandIn::LowBattery)
        }
    );
    let small = file(SMALL);
    let on_small = Now {
        loaded: Some((&small, Some(StandIn::LowBattery))),
        ..low
    };
    assert_eq!(
        plan(on_small),
        Plan::Keep,
        "it stays while the battery is low"
    );
    assert_eq!(
        plan(Now {
            low_battery: false,
            ..on_small
        }),
        Plan::Load {
            model: home.clone(),
            stand_in: None
        },
        "the pick comes back once the battery is fine"
    );
}

#[test]
fn a_model_in_use_kept_loaded_or_just_swapped_is_not_swapped() {
    let disk = disk();
    let home = file(BIG);
    let low = Now {
        loaded: Some((&home, None)),
        low_battery: true,
        ..now(&home, &disk)
    };
    assert_eq!(
        plan(Now {
            in_flight: 1,
            ..low
        }),
        Plan::Keep,
        "a request in flight keeps its model"
    );
    assert_eq!(
        plan(Now {
            secs_since_swap: SWAP_COOLDOWN_SECS - 1,
            ..low
        }),
        Plan::Keep,
        "no second swap inside the cooldown"
    );
    assert_eq!(
        plan(Now {
            kept_loaded: true,
            ..low
        }),
        Plan::Keep
    );
    assert_eq!(
        plan(Now {
            forced: true,
            ..low
        }),
        Plan::Keep
    );
    let smallest = file(SMALL);
    assert_eq!(
        plan(Now {
            home: &smallest,
            loaded: Some((&smallest, None)),
            ..low
        }),
        Plan::Keep,
        "nothing smaller than the pick is on disk"
    );
}

#[test]
fn a_pick_that_ran_out_of_memory_gets_the_largest_smaller_model_until_the_next_unload() {
    let disk = disk();
    let home = file(BIG);
    assert_eq!(out_of_memory_stand_in(&home, &disk), Some(file(MID)));
    assert_eq!(out_of_memory_stand_in(&file(SMALL), &disk), None);
    assert_eq!(
        out_of_memory_stand_in(Path::new("/x/custom.gguf"), &disk),
        None
    );
    let mid = file(MID);
    let standing = Now {
        loaded: Some((&mid, Some(StandIn::OutOfMemory))),
        ..now(&home, &disk)
    };
    assert_eq!(
        plan(standing),
        Plan::Keep,
        "no retry of the start that failed"
    );
    assert_eq!(
        plan(Now {
            low_battery: true,
            ..standing
        }),
        Plan::Load {
            model: file(SMALL),
            stand_in: Some(StandIn::LowBattery)
        }
    );
}

#[test]
fn the_idle_timer_grows_after_quick_reloads_and_shrinks_after_late_ones() {
    let base = 600;
    assert_eq!(learn(600, base, 30), 1_200);
    assert_eq!(learn(2_400, base, 5), LEARNED_MAX_SECS);
    assert_eq!(
        learn(1_200, base, 10 * 60),
        1_200,
        "an ordinary gap changes nothing"
    );
    assert_eq!(learn(1_200, base, RELOAD_LATE_SECS + 1), 600);
    assert_eq!(
        learn(600, base, RELOAD_LATE_SECS + 1),
        600,
        "never below the default"
    );

    let dir = tmp("learn");
    let model = file(BIG);
    assert_eq!(learned_secs(&dir, &model, base), base);
    save_learned(&dir, &model, 1_200).unwrap();
    save_learned(&dir, &file(SMALL), 2_400).unwrap();
    assert_eq!(learned_secs(&dir, &model, base), 1_200);
    assert_eq!(learned_secs(&dir, &file(SMALL), base), 2_400);
}

#[test]
fn only_downloaded_chat_models_count_as_on_disk() {
    let dir = tmp("disk");
    let chat = m(MID);
    std::fs::write(grokhub_core::model_download::final_path(&dir, chat), b"x").unwrap();
    let found = on_disk(&[dir.clone(), dir.join("missing")]);
    assert_eq!(
        found.iter().map(|(m, _)| m.id).collect::<Vec<_>>(),
        vec![chat.id]
    );
}

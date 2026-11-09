use super::super::local_scale;
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
        need: Need::Normal,
        scale: Scale::default(),
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
        Plan::Later {
            model: home.clone(),
            stand_in: None
        },
        "the pick comes back once the battery is fine, after the reply on the small one"
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

fn free_room() -> local_scale::Room {
    local_scale::Room {
        free_vram_mb: Some(16_000),
        free_ram_mb: 32_000,
        cpu_busy_pct: Some(10),
        on_battery: false,
        unified: false,
    }
}

fn with_room(room: local_scale::Room) -> Scale {
    Scale {
        room: Some(room),
        ..Scale::default()
    }
}

#[test]
fn a_hard_task_on_a_free_computer_steps_up_to_the_biggest_model_that_fits() {
    let disk = disk();
    let home = file(MID);
    let hard = Now {
        need: Need::Hard,
        scale: with_room(free_room()),
        ..now(&home, &disk)
    };
    assert_eq!(
        plan(hard),
        Plan::Load {
            model: file(BIG),
            stand_in: Some(StandIn::Performance)
        },
        "nothing runs: the bigger model starts at once"
    );
    assert_eq!(
        plan(Now {
            loaded: Some((&home, None)),
            ..hard
        }),
        Plan::Later {
            model: file(BIG),
            stand_in: Some(StandIn::Performance)
        },
        "the pick runs: the reply starts on it and the bigger model loads after"
    );
    assert_eq!(
        plan(Now {
            need: Need::Normal,
            ..hard
        }),
        Plan::Load {
            model: home.clone(),
            stand_in: None
        },
        "an everyday task gets the pick"
    );
    let busy = local_scale::Room {
        cpu_busy_pct: Some(80),
        ..free_room()
    };
    assert_eq!(
        plan(Now {
            scale: with_room(busy),
            ..hard
        }),
        Plan::Load {
            model: home.clone(),
            stand_in: None
        },
        "a busy CPU gets no step up"
    );
    let battery = local_scale::Room {
        on_battery: true,
        ..free_room()
    };
    assert_eq!(
        plan(Now {
            scale: with_room(battery),
            ..hard
        }),
        Plan::Load {
            model: home.clone(),
            stand_in: None
        },
        "on battery there is no step up"
    );
    let small = local_scale::Room {
        free_vram_mb: Some(4_000),
        free_ram_mb: 6_000,
        ..free_room()
    };
    assert_eq!(
        plan(Now {
            scale: with_room(small),
            ..hard
        }),
        Plan::Load {
            model: home.clone(),
            stand_in: None
        },
        "no bigger model fits with headroom"
    );
}

#[test]
fn a_bigger_model_stays_between_asks_and_steps_down_when_the_computer_gets_busy() {
    let disk = disk();
    let home = file(MID);
    let big = file(BIG);
    let on_big = Now {
        loaded: Some((&big, Some(StandIn::Performance))),
        scale: with_room(free_room()),
        ..now(&home, &disk)
    };
    assert_eq!(plan(on_big), Plan::Keep, "a light ask in between keeps it");
    let busy = local_scale::Room {
        cpu_busy_pct: Some(80),
        ..free_room()
    };
    assert_eq!(
        plan(Now {
            scale: with_room(busy),
            ..on_big
        }),
        Plan::Load {
            model: home.clone(),
            stand_in: None
        }
    );
    let scale = with_room(busy);
    assert!(local_scale::step_down_due(m(BIG), &scale));
    assert!(!local_scale::step_down_due(m(BIG), &with_room(free_room())));
    assert!(
        !local_scale::step_down_due(m(BIG), &Scale::default()),
        "no reading: it stays"
    );
}

#[test]
fn a_game_runs_the_lightest_model_and_saving_power_lightens_light_tasks() {
    let disk = disk();
    let home = file(BIG);
    let gaming = Now {
        need: Need::Hard,
        scale: Scale {
            gaming: true,
            ..with_room(free_room())
        },
        ..now(&home, &disk)
    };
    assert_eq!(
        plan(gaming),
        Plan::Load {
            model: file(SMALL),
            stand_in: Some(StandIn::Gaming)
        },
        "a game gets the lightest model even for a hard task"
    );
    let saving = Now {
        scale: Scale {
            saving: true,
            ..Scale::default()
        },
        ..now(&home, &disk)
    };
    assert_eq!(
        plan(Now {
            need: Need::Light,
            ..saving
        }),
        Plan::Load {
            model: file(SMALL),
            stand_in: Some(StandIn::Lighter)
        }
    );
    assert_eq!(
        plan(saving),
        Plan::Load {
            model: file(SMALL),
            stand_in: Some(StandIn::Lighter)
        },
        "an everyday task gets the largest model at half the pick or less"
    );
    assert_eq!(
        plan(Now {
            need: Need::Hard,
            ..saving
        }),
        Plan::Load {
            model: home.clone(),
            stand_in: None
        },
        "a hard task keeps the pick"
    );
    assert_eq!(
        plan(Now {
            kept_loaded: true,
            ..gaming
        }),
        Plan::Load {
            model: home.clone(),
            stand_in: None
        },
        "keep loaded holds the pick"
    );
}

#[test]
fn less_free_memory_than_the_pick_needs_runs_the_largest_model_that_fits() {
    let disk = disk();
    let home = file(BIG);
    let tight = local_scale::Room {
        free_vram_mb: None,
        free_ram_mb: 9_000,
        ..free_room()
    };
    assert_eq!(
        plan(Now {
            scale: with_room(tight),
            ..now(&home, &disk)
        }),
        Plan::Load {
            model: file(MID),
            stand_in: Some(StandIn::Tight)
        }
    );
}

#[test]
fn the_mode_label_follows_the_game_and_the_stand_in() {
    let gaming = Scale {
        gaming: true,
        ..Scale::default()
    };
    assert_eq!(mode(None, &gaming), local_scale::Mode::Gaming);
    assert_eq!(
        mode(Some(StandIn::Performance), &Scale::default()),
        local_scale::Mode::Performance
    );
    assert_eq!(
        mode(Some(StandIn::Lighter), &Scale::default()),
        local_scale::Mode::Balanced
    );
}

#[test]
fn background_work_keeps_whatever_is_loaded() {
    let disk = disk();
    let home = file(BIG);
    let small = file(SMALL);
    let base = Now {
        need: Need::Background,
        ..now(&home, &disk)
    };
    // The small stand-in stays, though the pick would come back for a chat.
    let on_small = Now {
        loaded: Some((&small, Some(StandIn::LowBattery))),
        ..base
    };
    assert_eq!(plan(on_small), Plan::Keep);
    assert_eq!(
        plan(Now {
            need: Need::Normal,
            ..on_small
        }),
        Plan::Later {
            model: home.clone(),
            stand_in: None
        }
    );
    // A low battery doesn't swap the loaded pick out for it either.
    assert_eq!(
        plan(Now {
            loaded: Some((&home, None)),
            low_battery: true,
            ..base
        }),
        Plan::Keep
    );
}

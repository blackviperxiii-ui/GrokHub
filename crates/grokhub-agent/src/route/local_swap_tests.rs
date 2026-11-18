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
        paired: &[],
        loaded_paired: false,
        refused: &[],
        low_battery: false,
        kept_loaded: false,
        forced: false,
        in_flight: 0,
        secs_since_swap: u64::MAX,
        need: Need::Normal,
        scale: Scale::default(),
        task: None,
        dynamic: true,
        marks: &NO_MARKS,
    }
}

static NO_MARKS: Marks = Marks::new();

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
    assert!(local_scale::step_down_due(m(BIG).run_mb, &scale));
    assert!(!local_scale::step_down_due(
        m(BIG).run_mb,
        &with_room(free_room())
    ));
    assert!(
        !local_scale::step_down_due(m(BIG).run_mb, &Scale::default()),
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
fn a_pick_that_fits_only_without_its_drafter_steps_down_to_a_pair_that_fits() {
    let disk = disk();
    let home = file(BIG);
    // 11 GB free RAM: 8,800 MB usable.
    let room = local_scale::Room {
        free_vram_mb: None,
        free_ram_mb: 11_000,
        ..free_room()
    };
    // The 12B pair needs 9,477 MB; the E4B pair 6,288 fits.
    let paired = Now {
        scale: with_room(room),
        paired: &[BIG, MID],
        ..now(&home, &disk)
    };
    assert_eq!(
        plan(paired),
        Plan::Load {
            model: file(MID),
            stand_in: Some(StandIn::Tight)
        }
    );
    // Without its drafter on disk the 12B alone (8,399 MB) fits.
    assert_eq!(
        plan(Now {
            paired: &[],
            ..paired
        }),
        Plan::Load {
            model: home.clone(),
            stand_in: None
        }
    );
    // Stopping the running 12B frees its drafter too, so it stays.
    assert_eq!(
        plan(Now {
            loaded: Some((&home, None)),
            loaded_paired: true,
            scale: with_room(local_scale::Room {
                free_ram_mb: 1_000,
                ..room
            }),
            ..paired
        }),
        Plan::Keep
    );
}

#[test]
fn the_planner_tries_a_pair_the_launch_may_load_and_leaves_the_call_to_it() {
    let disk = disk();
    let home = file(BIG);
    // A 12 GB card with 11,000 MB free and 1,000 MB of RAM free: 11,800 MB
    // usable. The 12B pair (9,477 MB at its default context) is over the
    // launch's 8,800 MB GPU budget, but the launch may still load it at a
    // shorter context, so the planner tries the pick.
    let room = local_scale::Room {
        free_vram_mb: Some(11_000),
        free_ram_mb: 1_000,
        ..free_room()
    };
    let n = Now {
        scale: with_room(room),
        paired: &[BIG, MID, SMALL],
        ..now(&home, &disk)
    };
    assert_eq!(
        plan(n),
        Plan::Load {
            model: home.clone(),
            stand_in: None
        }
    );
    // A 12B that runs without its drafter frees only itself (8,399 MB): with
    // 1,000 MB free RAM and no VRAM reading the pair no longer fits.
    let tight = local_scale::Room {
        free_vram_mb: None,
        free_ram_mb: 1_000,
        ..free_room()
    };
    let running_alone = Now {
        loaded: Some((&home, None)),
        scale: with_room(tight),
        paired: &[BIG, MID],
        ..now(&home, &disk)
    };
    assert_eq!(
        plan(running_alone),
        Plan::Load {
            model: file(MID),
            stand_in: Some(StandIn::Tight)
        }
    );
    assert_eq!(
        plan(Now {
            loaded_paired: true,
            ..running_alone
        }),
        Plan::Keep
    );
}

#[test]
fn a_stand_in_for_a_refused_pair_stays_while_the_refusal_holds() {
    let disk = disk();
    let home = file(BIG);
    // The planner thinks the 12B fits (no reading), but its launch refused
    // the pair and the E4B stood in.
    let refused = [home.clone()];
    let n = Now {
        loaded: Some((&file(MID), Some(StandIn::Tight))),
        paired: &[BIG, MID],
        ..now(&home, &disk)
    };
    assert_eq!(
        plan(n),
        Plan::Later {
            model: home.clone(),
            stand_in: None
        },
        "without the hold it reloads into the same refusal"
    );
    assert_eq!(
        plan(Now {
            refused: &refused,
            ..n
        }),
        Plan::Keep
    );
}

#[test]
fn a_pair_hold_ends_on_a_new_governor_state_or_more_room_in_its_own_pool() {
    use super::super::local_autoconfig::{FreeNow, GovState, PairRefusal, Pool};
    let free = |vram: u64, ram: u64| FreeNow {
        vram_free_mb: Some(vram),
        ram_free_mb: Some(ram),
    };
    let on = |state: GovState, f: Option<FreeNow>| HoldNow { state, free: f };
    // Refused on a GPU with 6,000 MB of VRAM and 10,000 MB of RAM free.
    let gpu = Hold::Pair(PairRefusal {
        pool: Pool::Vram,
        free_mb: Some(6_000),
        state: GovState::Normal,
        on_gpu: true,
    });
    assert!(held(&gpu, &on(GovState::Normal, Some(free(6_000, 10_000)))));
    assert!(held(&gpu, &on(GovState::Normal, None)), "no reading");
    assert!(held(&gpu, &on(GovState::Normal, Some(free(8_047, 10_000)))));
    assert!(!held(
        &gpu,
        &on(GovState::Normal, Some(free(8_048, 10_000)))
    ));
    // RAM grew but the pair needed VRAM: still held.
    assert!(held(&gpu, &on(GovState::Normal, Some(free(6_000, 30_000)))));
    // The governor state changed (Throttle back to Normal, say): released.
    assert!(!held(
        &gpu,
        &on(GovState::Throttle, Some(free(6_000, 10_000)))
    ));
    assert!(!held(&gpu, &on(GovState::RampUp, None)));
    // A GPU pair refused with no VRAM reading has no baseline: neither
    // more RAM nor a later VRAM reading releases it; a new state does.
    let blind = Hold::Pair(PairRefusal {
        pool: Pool::Vram,
        free_mb: None,
        state: GovState::Normal,
        on_gpu: true,
    });
    assert!(held(
        &blind,
        &on(GovState::Normal, Some(free(30_000, 60_000)))
    ));
    assert!(!held(&blind, &on(GovState::Throttle, None)));
    // A pair sized in RAM (no GPU) is released by RAM.
    let ram = Hold::Pair(PairRefusal {
        pool: Pool::Ram,
        free_mb: Some(10_000),
        state: GovState::Normal,
        on_gpu: false,
    });
    assert!(held(
        &ram,
        &on(GovState::Normal, Some(free(20_000, 12_047)))
    ));
    assert!(!held(&ram, &on(GovState::Normal, Some(free(0, 12_048)))));
    // Out of memory holds until the next unload, whatever happens.
    let oom = Hold::UntilUnload;
    assert!(held(
        &oom,
        &on(GovState::Throttle, Some(free(90_000, 90_000)))
    ));
    let r = Refusals {
        models: vec![(file(BIG), gpu), (file(MID), oom)],
        since_ms: 0,
    };
    assert_eq!(
        r.holding(&on(GovState::Normal, Some(free(6_000, 10_000)))),
        vec![file(BIG), file(MID)]
    );
    assert_eq!(
        r.holding(&on(GovState::Normal, Some(free(9_000, 10_000)))),
        vec![file(MID)]
    );
}

#[test]
fn the_mode_label_follows_the_game_and_the_stand_in() {
    let gaming = Scale {
        gaming: true,
        ..Scale::default()
    };
    assert_eq!(mode(None, &gaming, true), local_scale::Mode::Gaming);
    assert_eq!(
        mode(Some(StandIn::Performance), &Scale::default(), true),
        local_scale::Mode::Performance
    );
    assert_eq!(
        mode(Some(StandIn::Lighter), &Scale::default(), true),
        local_scale::Mode::Balanced
    );
    assert_eq!(
        mode(None, &Scale::default(), false),
        local_scale::Mode::Fixed
    );
    assert_eq!(mode(None, &gaming, false), local_scale::Mode::Gaming);
}

const REASONER: &str = "deepseek-r1-0528-qwen3-8b-heretic-q4_k_m";

fn task_disk() -> Vec<(&'static LocalModel, PathBuf)> {
    [BIG, REASONER, SMALL]
        .iter()
        .map(|id| (m(id), file(id)))
        .collect()
}

#[test]
fn with_dynamic_on_a_task_runs_on_the_model_best_at_it_and_chat_stays_on_the_pick() {
    let disk = task_disk();
    let home = file(BIG);
    let n = Now {
        task: Some(TaskKind::Reason),
        ..now(&home, &disk)
    };
    assert_eq!(
        plan(n),
        Plan::Load {
            model: file(REASONER),
            stand_in: Some(StandIn::BestFor(TaskKind::Reason))
        },
        "nothing runs: the reasoner starts for a reasoning task"
    );
    let on_home = Now {
        loaded: Some((&home, None)),
        ..n
    };
    assert_eq!(
        plan(on_home),
        Plan::Later {
            model: file(REASONER),
            stand_in: Some(StandIn::BestFor(TaskKind::Reason))
        },
        "the pick runs: the reply stays on it and the reasoner loads when the chat is quiet"
    );
    let chat = Now {
        task: Some(TaskKind::Chat),
        ..on_home
    };
    assert_eq!(plan(chat), Plan::Keep, "everyday chat stays on the pick");
    let off = Now {
        dynamic: false,
        ..on_home
    };
    assert_eq!(
        plan(off),
        Plan::Keep,
        "Dynamic off: the pick answers everything"
    );
}

#[test]
fn a_model_picked_for_a_task_stays_for_chat_and_a_game_still_wins() {
    let disk = task_disk();
    let home = file(BIG);
    let reasoner = file(REASONER);
    let on_reasoner = Now {
        loaded: Some((&reasoner, Some(StandIn::BestFor(TaskKind::Reason)))),
        task: Some(TaskKind::Chat),
        ..now(&home, &disk)
    };
    assert_eq!(plan(on_reasoner), Plan::Keep, "no reload back and forth");
    let off = Now {
        dynamic: false,
        ..on_reasoner
    };
    assert_eq!(
        plan(off),
        Plan::Later {
            model: home.clone(),
            stand_in: None
        },
        "turning Dynamic off brings the (bigger) pick back once the chat is quiet"
    );
    let game = Now {
        scale: Scale {
            gaming: true,
            ..Scale::default()
        },
        task: Some(TaskKind::Reason),
        ..on_reasoner
    };
    assert_eq!(
        plan(game),
        Plan::Load {
            model: file(SMALL),
            stand_in: Some(StandIn::Gaming)
        }
    );
}

#[test]
fn a_model_that_kept_missing_a_task_is_no_longer_picked_for_it() {
    let disk = task_disk();
    let home = file(BIG);
    let mut marks = Marks::new();
    marks.note(m(REASONER).id, TaskKind::Reason, false);
    let n = Now {
        loaded: Some((&home, None)),
        task: Some(TaskKind::Reason),
        marks: &marks,
        ..now(&home, &disk)
    };
    assert_eq!(plan(n), Plan::Keep, "one miss puts it inside the margin");
}

#[test]
fn the_row_says_what_the_task_model_is_for() {
    assert_eq!(
        StandIn::BestFor(TaskKind::Code).line(),
        "Running a model that's stronger at coding than your pick. Turn off Dynamic to always use your pick."
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

#[test]
fn the_gaming_model_is_the_one_a_game_would_swap_to() {
    let disk = disk();
    let home = file(BIG);
    let n = Now {
        scale: Scale {
            gaming: true,
            ..Scale::default()
        },
        ..now(&home, &disk)
    };
    let Plan::Load { model, .. } = plan(n) else {
        panic!("a game with nothing loaded starts a model");
    };
    assert_eq!(gaming_model(&home, &disk), Some(model));
    assert_eq!(gaming_model(&file(SMALL), &disk), None, "nothing lighter");
    assert_eq!(gaming_model(Path::new("/models/unknown.gguf"), &disk), None);
}

#[test]
fn a_game_takes_the_lightest_gaming_safe_model_over_a_lighter_one_that_isnt() {
    let safe = m(SMALL);
    assert!(
        safe.profile.gaming_safe,
        "the catalog marks e2b gaming-safe"
    );
    let weak: &'static LocalModel = Box::leak(Box::new(LocalModel {
        id: "weak",
        file: "weak.gguf",
        run_mb: 1_000,
        profile: local_setup::LocalProfile {
            gaming_safe: false,
            ..safe.profile
        },
        ..*safe
    }));
    let disk = vec![
        (weak, PathBuf::from("/models/weak.gguf")),
        (safe, file(SMALL)),
    ];
    assert_eq!(gaming_below(m(BIG), &disk), Some(file(SMALL)));
    assert_eq!(
        gaming_below(m(BIG), &disk[..1]),
        Some(PathBuf::from("/models/weak.gguf")),
        "with no gaming-safe model, the lightest still stands in"
    );
}

use super::*;

const E2B: &str = "Huihui-gemma-4-E2B-it-qat-q4_0-unquantized-abliterated-Q4_K.gguf";
const G12B: &str = "Huihui-gemma-4-12B-it-qat-q4_0-unquantized-abliterated-Q4_K.gguf";
const G26B: &str = "Huihui-gemma-4-26B-A4B-it-qat-q4_0-unquantized-abliterated-Q4_K.gguf";
const G26B_MTP: &str = "Huihui-gemma-4-26B-A4B-it-abliterated-MTP-BF16.gguf";
const G12B_MTP: &str = "Huihui-gemma-4-12B-it-abliterated-MTP-BF16.gguf";

/// Lines of llama.cpp b11602's `llama-server --help` (the build pinned before b12017).
const B11602_HELP: &str = "\
--spec-draft-n-max N                    number of tokens to draft for speculative decoding (default: 3)
                                        (env: LLAMA_ARG_SPEC_DRAFT_N_MAX)
--spec-draft-ngl, -ngld, --gpu-layers-draft, --n-gpu-layers-draft N
                                        max. number of draft model layers to store in VRAM, either an exact
                                        number, 'auto', or 'all' (default: auto)
--spec-draft-model, -md, --model-draft FNAME
                                        draft model for speculative decoding (default: unused)
--spec-type none,draft-simple,draft-eagle3,draft-mtp,draft-dflash,draft-dspark,ngram-simple
                                        comma-separated list of types of speculative decoding to use (default:
                                        none)
--spec-ngram-map-k4v-min-hits N         minimum hits for ngram-map-k4v speculative decoding (default: 1)
--draft, --draft-n, --draft-max N       the argument has been removed. use --spec-draft-n-max or
                                        --spec-ngram-mod-n-max
--draft-min, --draft-n-min N            the argument has been removed. use --spec-draft-n-min or
";

/// The whole `llama-server --help` of the pinned llama.cpp b12017.
const B12017_HELP: &str = include_str!("../tests/fixtures/llama_server_help_b12017.txt");

/// An older engine, before `--spec-*`.
const OLD_HELP: &str = "\
-md,   --model-draft FNAME              draft model for speculative decoding (default: unused)
-ngld, --gpu-layers-draft, --n-gpu-layers-draft N
                                        number of layers to store in VRAM for the draft model
--draft-max, --draft, --draft-n N       number of tokens to draft for speculative decoding (default: 16)
";

#[test]
fn b11602_uses_the_spec_flags_and_skips_removed_ones() {
    let f = parse_help(B11602_HELP);
    assert_eq!(
        f,
        DraftFlags {
            model: Some("--spec-draft-model".into()),
            ngl: Some("--spec-draft-ngl".into()),
            max: Some("--spec-draft-n-max".into()),
            spec_type: Some("--spec-type".into()),
            kinds: [
                "none",
                "draft-simple",
                "draft-eagle3",
                "draft-mtp",
                "draft-dflash",
                "draft-dspark",
                "ngram-simple"
            ]
            .map(String::from)
            .to_vec(),
        }
    );
    assert!(f.supports(DraftKind::Simple));
    assert!(f.supports(DraftKind::Dflash));
    assert!(f.supports(DraftKind::Mtp));
}

#[test]
fn b12017_drafts_with_the_spec_flags_and_lists_every_kind() {
    let f = parse_help(B12017_HELP);
    assert_eq!(
        f,
        DraftFlags {
            model: Some("--spec-draft-model".into()),
            ngl: Some("--spec-draft-ngl".into()),
            max: Some("--spec-draft-n-max".into()),
            spec_type: Some("--spec-type".into()),
            kinds: [
                "none",
                "draft-simple",
                "draft-eagle3",
                "draft-mtp",
                "draft-dflash",
                "draft-dspark",
                "ngram-simple",
                "ngram-map-k",
                "ngram-map-k4v",
                "ngram-mod",
                "ngram-cache"
            ]
            .map(String::from)
            .to_vec(),
        }
    );
    assert!(f.supports(DraftKind::Simple));
    assert!(f.supports(DraftKind::Dflash));
    assert!(f.supports(DraftKind::Mtp));
}

#[test]
fn b12017_help_marks_the_removed_and_deprecated_flags() {
    let all = help_flags(B12017_HELP);
    let retired: Vec<&str> = all
        .iter()
        .filter(|(_, r)| **r)
        .map(|(n, _)| n.as_str())
        .collect();
    assert_eq!(
        retired,
        [
            "--defrag-thold",
            "--draft",
            "--draft-max",
            "--draft-min",
            "--draft-n",
            "--draft-n-min",
            "--spec-ngram-min-hits",
            "--spec-ngram-size-m",
            "--spec-ngram-size-n",
            "-dt"
        ]
    );
    // The flags the draft picks use are live; section rules name nothing.
    for live in [
        "--spec-draft-model",
        "--spec-draft-ngl",
        "--spec-draft-n-max",
        "--spec-type",
        "--reasoning-effort",
        "--api-key",
    ] {
        assert_eq!(all.get(live), Some(&false), "{live}");
    }
    assert!(!all.keys().any(|n| n.starts_with("---")));
    let f = parse_help(B12017_HELP);
    for picked in [&f.model, &f.ngl, &f.max, &f.spec_type] {
        let name = picked.as_deref().expect("picked");
        assert_eq!(all.get(name), Some(&false), "{name}");
    }
}

#[test]
fn a_notice_further_down_the_description_still_retires_the_flag() {
    let help = "\
--old-thing N                           what it did
                                        (env: LLAMA_ARG_OLD)
                                        the argument has been removed. use --new-thing
--new-thing N                           what it does
";
    assert_eq!(
        help_flags(help).into_iter().collect::<Vec<_>>(),
        [
            ("--new-thing".to_string(), false),
            ("--old-thing".to_string(), true)
        ]
    );
}

#[test]
fn an_older_engine_drafts_with_md_only() {
    let f = parse_help(OLD_HELP);
    assert_eq!(f.model.as_deref(), Some("--model-draft"));
    assert_eq!(f.ngl.as_deref(), Some("--gpu-layers-draft"));
    assert_eq!(f.max.as_deref(), Some("--draft-max"));
    assert_eq!(f.spec_type, None);
    assert!(f.supports(DraftKind::Simple));
    assert!(!f.supports(DraftKind::Dflash));
    assert!(!f.supports(DraftKind::Mtp));
    let none = parse_help("--threads N   threads\n");
    assert_eq!(none, DraftFlags::default());
    assert!(!none.supports(DraftKind::Simple));
}

#[test]
fn every_gemma_pairs_with_its_own_mtp_heads() {
    let got: Vec<(&str, &str, DraftKind)> = pairs()
        .iter()
        .filter(|p| p.target.id.starts_with("gemma-4-"))
        .map(|p| (p.target.id, p.drafter.id, p.kind))
        .collect();
    // The other makers' small same-tokenizer drafters are plain drafts.
    let others: Vec<(&str, DraftKind)> = pairs()
        .iter()
        .filter(|p| !p.target.id.starts_with("gemma-4-"))
        .map(|p| (p.target.id, p.kind))
        .collect();
    assert_eq!(others.len(), 5, "{others:?}");
    assert!(
        others.iter().all(|(_, k)| *k == DraftKind::Simple),
        "{others:?}"
    );
    let want: Vec<(&str, &str, DraftKind)> = ["31b", "26b-a4b", "12b", "e4b", "e2b"]
        .iter()
        .map(|s| {
            let t = format!("gemma-4-{s}-it-abliterated-q4_k").leak() as &str;
            let d = format!("gemma-4-{s}-it-abliterated-mtp").leak() as &str;
            (t, d, DraftKind::Mtp)
        })
        .collect();
    assert_eq!(got, want);
    assert_eq!(pair_for_file(G12B).map(|p| p.drafter.file), Some(G12B_MTP));
    let e2b = pair_for_file(E2B).expect("even the smallest drafts");
    assert_eq!(
        pair_for_file(e2b.drafter.file),
        None,
        "a drafter has no drafter"
    );
    // A stock Gemma 4 file is not a catalog model, so it has no catalog pair.
    assert_eq!(pair_for_file("gemma-4-12B-it-Q4_K_M.gguf"), None);
    assert_eq!(pair_for_file("qwen2.5-7b-instruct-q4_k_m.gguf"), None);
    // A drafter row whose id says DFlash drafts as DFlash.
    let e2b = crate::local_setup::model_by_id("gemma-4-e2b-it-abliterated-q4_k").unwrap();
    let dflash = LocalModel {
        id: "test-dflash-drafter",
        ..*e2b
    };
    assert_eq!(DraftKind::of(e2b), DraftKind::Simple);
    assert_eq!(DraftKind::of(&dflash), DraftKind::Dflash);
    assert_eq!(DraftKind::Simple.spec_type(), "draft-simple");
    assert_eq!(DraftKind::Dflash.spec_type(), "draft-dflash");
    assert_eq!(DraftKind::Mtp.spec_type(), "draft-mtp");
}

#[test]
fn the_tokenizer_gate_needs_two_equal_hashes() {
    assert!(same_tokenizer(Some("ab"), Some("ab")));
    assert!(!same_tokenizer(Some("ab"), Some("cd")));
    assert!(!same_tokenizer(None, Some("ab")));
    assert!(!same_tokenizer(None, None));
}

#[test]
fn the_bench_keeps_a_drafter_at_40_percent_and_10_percent_faster() {
    let plain = Timing {
        tok_s: 20.0,
        drafted: 0,
        accepted: 0,
    };
    let fast = Timing {
        tok_s: 28.0,
        drafted: 200,
        accepted: 124,
    };
    let v = judge(&plain, &fast);
    assert!(v.keep);
    assert!((v.acceptance - 0.62).abs() < 1e-9);
    assert!((v.gain - 1.4).abs() < 1e-9);
    assert_eq!(
        verdict_line("Gemma 4 E2B", &v),
        "Gemma 4 E2B made replies 1.4× as fast (62% of its guesses were kept), so it stays on."
    );
    // Accepted often but barely faster.
    let flat = judge(
        &plain,
        &Timing {
            tok_s: 21.0,
            drafted: 200,
            accepted: 150,
        },
    );
    assert!(!flat.keep);
    assert_eq!(
        verdict_line("Gemma 4 E2B", &flat),
        "Gemma 4 E2B made replies 1.1× as fast with 75% of its guesses kept, so Loadout left it off."
    );
    // Faster, but under 40% accepted.
    let wild = judge(
        &plain,
        &Timing {
            tok_s: 30.0,
            drafted: 200,
            accepted: 70,
        },
    );
    assert!(!wild.keep);
    // Exactly at both bars.
    let edge = judge(
        &plain,
        &Timing {
            tok_s: 22.0,
            drafted: 100,
            accepted: 40,
        },
    );
    assert!(edge.keep, "{edge:?}");
    // Nothing drafted is nothing accepted.
    assert!(!judge(&plain, &plain).keep);
}

#[test]
fn timings_come_from_the_servers_reply() {
    let body = r#"{"choices":[{"message":{"content":"hi"}}],"timings":{"predicted_n":128,"predicted_per_second":31.5,"draft_n":96,"draft_n_accepted":61}}"#;
    assert_eq!(
        parse_timings(body),
        Some(Timing {
            tok_s: 31.5,
            drafted: 96,
            accepted: 61
        })
    );
    assert_eq!(
        parse_timings(r#"{"timings":{"predicted_per_second":12.0}}"#),
        Some(Timing {
            tok_s: 12.0,
            drafted: 0,
            accepted: 0
        })
    );
    assert_eq!(parse_timings(r#"{"choices":[]}"#), None);
}

#[test]
fn verdicts_are_kept_per_device_and_pair() {
    let dir = std::env::temp_dir().join(format!("grokhub-draft-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    assert!(load_verdicts(&dir).is_empty());
    let key = verdict_key(
        "cuda:RTX 4070",
        "gemma-4-12B-it-Q4_K_M.gguf",
        "gemma-4-E2B-it-Q4_K_M.gguf",
    );
    assert_eq!(
        key,
        "cuda:RTX 4070|gemma-4-12B-it-Q4_K_M.gguf|gemma-4-E2B-it-Q4_K_M.gguf"
    );
    let v = Verdict {
        keep: true,
        acceptance: 0.6,
        gain: 1.3,
        retest_at: None,
    };
    save_verdict(&dir, &key, v).unwrap();
    save_verdict(&dir, "cpu|a|b", Verdict { keep: false, ..v }).unwrap();
    let all = load_verdicts(&dir);
    assert_eq!(all.get(&key), Some(&v));
    assert_eq!(all.get("cpu|a|b").map(|v| v.keep), Some(false));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn decide_checks_engine_then_level_then_tokenizer_then_room_then_the_test() {
    let ok = Check {
        level_ok: true,
        engine_ok: true,
        tokenizer_ok: true,
        fits: true,
        verdict: Some(true),
    };
    assert_eq!(decide(ok), Decision::On);
    assert_eq!(
        decide(Check {
            verdict: None,
            ..ok
        }),
        Decision::NeedsTest
    );
    assert_eq!(
        decide(Check {
            verdict: Some(false),
            ..ok
        }),
        Decision::Off("the speed test found the draft model didn't help")
    );
    assert_eq!(
        decide(Check { fits: false, ..ok }),
        Decision::Off("there isn't room for the model and its draft model together")
    );
    assert_eq!(
        decide(Check {
            tokenizer_ok: false,
            ..ok
        }),
        Decision::Off("the draft model's tokenizer doesn't match the model")
    );
    assert_eq!(
        decide(Check {
            level_ok: false,
            ..ok
        }),
        Decision::Off("the machine is busy, so the draft model is off")
    );
    assert_eq!(
        decide(Check {
            engine_ok: false,
            level_ok: false,
            ..ok
        }),
        Decision::Off("this engine can't run a draft model")
    );
}

#[test]
fn a_drafter_is_offered_until_it_is_downloaded() {
    let dir = std::env::temp_dir().join(format!("grokhub-drafters-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        offer(G26B, &dir).map(|m| m.id),
        Some("gemma-4-26b-a4b-it-abliterated-mtp")
    );
    assert_eq!(offer(G26B_MTP, &dir), None, "a drafter isn't offered one");
    // A stock Gemma 4 file left from an older catalog is offered nothing.
    assert_eq!(offer("gemma-4-26B-A4B-it-Q4_K_M.gguf", &dir), None);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(G26B_MTP), b"x").unwrap();
    assert_eq!(offer(G26B, &dir), None);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn drafter_dirs_search_beside_then_shared_root_then_old_folder() {
    let model = Path::new("/home/v/LM Studio/gemma/G12.gguf");
    let dirs = drafter_dirs(
        model,
        Some(Path::new("/home/v/GrokHub/models")),
        Some(Path::new("/home/v/.config/GrokHub/models/drafters")),
    );
    assert_eq!(
        dirs,
        vec![
            PathBuf::from("/home/v/LM Studio/gemma/drafters"),
            PathBuf::from("/home/v/GrokHub/models/drafters"),
            PathBuf::from("/home/v/.config/GrokHub/models/drafters"),
        ]
    );
}

#[test]
fn drafter_dirs_drop_repeats_when_the_model_sits_in_the_root() {
    let model = Path::new("/home/v/GrokHub/models/G12.gguf");
    let dirs = drafter_dirs(model, Some(Path::new("/home/v/GrokHub/models")), None);
    assert_eq!(dirs, vec![PathBuf::from("/home/v/GrokHub/models/drafters")]);
}

#[test]
fn live_replies_turn_a_drafter_off_only_after_twenty_poor_ones() {
    let poor = Timing {
        tok_s: 30.0,
        drafted: 100,
        accepted: 30,
    };
    let mut live = Live::default();
    // Replies that drafted nothing don't count.
    let plain = Timing {
        drafted: 0,
        accepted: 0,
        ..poor
    };
    assert_eq!(live.add(&plain, 0), None);
    for _ in 1..LIVE_REPLIES {
        assert_eq!(live.add(&poor, 0), None);
    }
    let v = live.add(&poor, 1_000).expect("off after twenty");
    assert!(!v.keep && (v.acceptance - 0.3).abs() < 1e-9);
    assert_eq!(v.retest_at, Some(1_000 + RETEST_SECS));
    assert!(!v.standing(1_000 + RETEST_SECS), "due for a new test");
    assert!(v.standing(1_000));
    // A good window stays on, and the count starts again.
    let good = Timing {
        accepted: 70,
        ..poor
    };
    for _ in 0..LIVE_REPLIES {
        assert_eq!(live.add(&good, 0), None);
    }
}

#[test]
fn a_verdict_saved_before_retests_still_loads_and_stands() {
    let v: Verdict = serde_json::from_str(r#"{"keep":true,"acceptance":0.6,"gain":1.3}"#).unwrap();
    assert_eq!(v.retest_at, None);
    assert!(v.standing(u64::MAX));
}

#[test]
fn the_unattended_speed_test_waits_for_a_quiet_machine() {
    let quiet = Quiet {
        calm: true,
        on_battery: false,
        game: false,
        user_idle_secs: Some(AUTO_TEST_IDLE_SECS),
        model_idle_secs: AUTO_TEST_IDLE_SECS,
        in_flight: 0,
        warm_wanted: false,
    };
    assert!(auto_test_due(&quiet, 100, 100));
    // An OS that can't tell input idle time still gets a test.
    assert!(auto_test_due(
        &Quiet {
            user_idle_secs: None,
            ..quiet
        },
        0,
        100
    ));
    for busy in [
        Quiet {
            calm: false,
            ..quiet
        },
        Quiet {
            on_battery: true,
            ..quiet
        },
        Quiet {
            game: true,
            ..quiet
        },
        Quiet {
            user_idle_secs: Some(30),
            ..quiet
        },
        Quiet {
            model_idle_secs: 30,
            ..quiet
        },
        Quiet {
            in_flight: 1,
            ..quiet
        },
        Quiet {
            warm_wanted: true,
            ..quiet
        },
    ] {
        assert!(!auto_test_due(&busy, 0, 100), "{busy:?}");
    }
    assert!(!auto_test_due(&quiet, 101, 100), "not before next_at");
}

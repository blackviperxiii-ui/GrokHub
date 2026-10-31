use super::*;
use Bucket::{Hidden as H, MaySlow as S, Recommended as R};

const ALL: [&str; 5] = [
    "Gemma 4 31B",
    "Gemma 4 26B-A4B",
    "Gemma 4 12B",
    "Gemma 4 E4B",
    "Gemma 4 E2B",
];

fn profile(gpus: &[u64], unified: bool, ram_mb: u64) -> Profile {
    Profile {
        gpus_vram_mb: gpus.to_vec(),
        unified,
        ram_mb,
        free_disk_mb: 500_000,
        simd: CpuSimd::Avx2,
        llama_build: Some(11_602),
    }
}

fn gpu(gpus: &[u64], ram_mb: u64) -> Profile {
    profile(gpus, false, ram_mb)
}

fn cpu_only(ram_mb: u64) -> Profile {
    profile(&[], false, ram_mb)
}

fn apple(ram_mb: u64) -> Profile {
    Profile {
        simd: CpuSimd::Neon,
        ..profile(&[], true, ram_mb)
    }
}

fn run(p: &Profile) -> Vec<Pick> {
    score(p, local_models(), &BenchCache::new())
}

fn buckets(picks: &[Pick]) -> Vec<Bucket> {
    assert_eq!(
        picks.iter().map(|x| x.model.name).collect::<Vec<_>>(),
        ALL,
        "every model but the drafter, biggest first"
    );
    picks.iter().map(|x| x.bucket).collect()
}

fn pre(picks: &[Pick]) -> Option<&'static str> {
    let on: Vec<_> = picks.iter().filter(|x| x.preselected).collect();
    assert!(on.len() <= 1, "at most one pre-selected model");
    on.first().map(|x| x.model.name)
}

fn why<'a>(picks: &'a [Pick], name: &str) -> &'a str {
    &picks
        .iter()
        .find(|x| x.model.name == name)
        .expect(name)
        .reason
}

#[test]
fn an_avx_only_cpu_with_6_gb_is_cloud_only_with_e2b_may_be_slow() {
    let p = Profile {
        simd: CpuSimd::AvxOnly,
        ..cpu_only(6_144)
    };
    let picks = run(&p);
    assert_eq!(buckets(&picks), [H, H, H, H, S]);
    assert_eq!(pre(&picks), None);
    assert_eq!(tier(&p), "T0");
    assert_eq!(
        why(&picks, "Gemma 4 E4B"),
        "Needs AVX2; this CPU has AVX only."
    );
    assert_eq!(
        why(&picks, "Gemma 4 31B"),
        "Needs AVX2; this CPU has AVX only."
    );
    assert_eq!(
        why(&picks, "Gemma 4 E2B"),
        "Needs 3 GB of spare RAM; this PC has 2 GB, so it may be slow."
    );
}

#[test]
fn an_avx_only_cpu_with_room_tops_out_at_e2b() {
    let p = Profile {
        simd: CpuSimd::AvxOnly,
        ..cpu_only(16_384)
    };
    let picks = run(&p);
    assert_eq!(buckets(&picks), [H, H, H, H, R]);
    assert_eq!(pre(&picks), Some("Gemma 4 E2B"));
    assert_eq!(tier(&p), "T1");
}

#[test]
fn a_cpu_without_avx_hides_local_ai() {
    let p = Profile {
        simd: CpuSimd::NoAvx,
        ..cpu_only(32_768)
    };
    let picks = run(&p);
    assert_eq!(buckets(&picks), [H; 5]);
    assert_eq!(pre(&picks), None);
    assert_eq!(tier(&p), "T0");
    assert_eq!(
        why(&picks, "Gemma 4 E2B"),
        "This CPU has no AVX, so local AI is off and Loadout uses the cloud."
    );
}

#[test]
fn an_8_gb_laptop_on_its_igpu_gets_e2b() {
    // Iris Xe / Vega iGPUs share RAM, so the probe leaves them out.
    let p = cpu_only(8_192);
    let picks = run(&p);
    assert_eq!(buckets(&picks), [H, H, H, S, R]);
    assert_eq!(pre(&picks), Some("Gemma 4 E2B"));
    assert_eq!(tier(&p), "T1");
    assert_eq!(
        why(&picks, "Gemma 4 E2B"),
        "Fits in 4 GB of spare RAM, about 7 tok/s."
    );
    assert_eq!(
        why(&picks, "Gemma 4 E4B"),
        "Needs 6 GB of spare RAM; this PC has 4 GB, so it may be slow."
    );
    assert_eq!(
        why(&picks, "Gemma 4 12B"),
        "Needs 8.2 GB of memory; this PC has 6 GB."
    );
}

#[test]
fn a_16_gb_pc_without_a_gpu_gets_e4b_and_12b_is_slow_on_the_cpu() {
    let p = cpu_only(16_384);
    let picks = run(&p);
    assert_eq!(buckets(&picks), [H, H, S, R, R]);
    assert_eq!(pre(&picks), Some("Gemma 4 E4B"));
    assert_eq!(tier(&p), "T2");
    assert_eq!(
        why(&picks, "Gemma 4 E4B"),
        "Fits in 12 GB of spare RAM, about 5 tok/s."
    );
    assert_eq!(
        why(&picks, "Gemma 4 12B"),
        "Runs on the CPU here at about 3 tok/s, so it may be slow."
    );
}

#[test]
fn a_4_gb_gpu_runs_e2b_and_e4b_needs_partial_offload() {
    // GTX 1050 Ti / RX 560 4 GB with 8 GB RAM.
    let p = gpu(&[4_096], 8_192);
    let picks = run(&p);
    assert_eq!(buckets(&picks), [H, H, S, S, R]);
    assert_eq!(pre(&picks), Some("Gemma 4 E2B"));
    assert_eq!(tier(&p), "T1");
    assert_eq!(
        why(&picks, "Gemma 4 E2B"),
        "Fits in 4 GB of GPU memory, about 60 tok/s."
    );
    assert_eq!(
        why(&picks, "Gemma 4 E4B"),
        "Needs 6 GB of GPU memory; this PC has 4 GB, so part of it runs on the CPU and it may be slow."
    );
}

#[test]
fn a_2_gb_gpu_falls_back_to_the_ram_budget() {
    // GTX 950 / GT 1030 2 GB: too small to hold a model, so RAM decides.
    let p = gpu(&[2_048], 8_192);
    assert_eq!(p.budget(), (Memory::System, 4_096));
    let picks = run(&p);
    assert_eq!(buckets(&picks), [H, H, H, S, R]);
    assert_eq!(pre(&picks), Some("Gemma 4 E2B"));
    assert_eq!(
        why(&picks, "Gemma 4 E2B"),
        "Fits in 4 GB of spare RAM, about 7 tok/s."
    );
}

#[test]
fn a_6_to_8_gb_gpu_gets_e4b() {
    for vram in [6_144, 8_192] {
        let p = gpu(&[vram], 16_384);
        let picks = run(&p);
        assert_eq!(buckets(&picks), [S, S, S, R, R], "{vram}");
        assert_eq!(pre(&picks), Some("Gemma 4 E4B"), "{vram}");
        assert_eq!(tier(&p), "T2");
    }
    let picks = run(&gpu(&[6_144], 16_384));
    assert_eq!(
        why(&picks, "Gemma 4 12B"),
        "Needs 10 GB of GPU memory; this PC has 6 GB, so part of it runs on the CPU and it may be slow."
    );
}

#[test]
fn an_11_to_12_gb_gpu_gets_12b() {
    // GTX 1080 Ti 11 GB, RTX 3060 12 GB.
    for vram in [11_264, 12_288] {
        let p = gpu(&[vram], 32_768);
        let picks = run(&p);
        assert_eq!(buckets(&picks), [S, S, R, R, R], "{vram}");
        assert_eq!(pre(&picks), Some("Gemma 4 12B"), "{vram}");
        assert_eq!(tier(&p), "T3");
    }
    let picks = run(&gpu(&[12_288], 32_768));
    assert_eq!(
        why(&picks, "Gemma 4 26B-A4B"),
        "Needs 15 GB of GPU memory; this PC has 12 GB, so part of it runs on the CPU and it may be slow."
    );
}

#[test]
fn the_example_amd_16_gb_desktop_is_t3_on_26b_a4b() {
    // RX 9070 XT 16 GB + Radeon iGPU (left out), Ryzen 9 9950X3D, 58 GB RAM.
    let p = gpu(&[16_304], 59_392);
    let picks = run(&p);
    assert_eq!(buckets(&picks), [S, R, R, R, R]);
    assert_eq!(pre(&picks), Some("Gemma 4 26B-A4B"));
    assert_eq!(tier(&p), "T3");
    assert_eq!(
        why(&picks, "Gemma 4 26B-A4B"),
        "Fits in 15.9 GB of GPU memory, about 60 tok/s."
    );
    assert_eq!(
        why(&picks, "Gemma 4 31B"),
        "Needs 20 GB of GPU memory; this PC has 15.9 GB, so part of it runs on the CPU and it may be slow."
    );
}

/// The catalog with an optional row that needs a newer engine put first, as
/// the biggest model. The shipped catalog has none, but the picker still
/// honours both columns.
fn with_optional_row() -> &'static [LocalModel] {
    let big = crate::local_setup::model_by_id("gemma-4-31b-it-abliterated-q4_k").expect("31B");
    let optional = LocalModel {
        id: "test-optional-30b",
        name: "Optional 30B",
        run_mb: 17_600,
        min_llama_build: Some(10_353),
        optional: true,
        drafter: None,
        min_budget: crate::local_setup::Budgets {
            gpu: Some(22_528),
            unified: Some(32_768),
            cpu: None,
        },
        tok_s: crate::local_setup::TokS {
            gpu: 40,
            unified: 30,
            cpu: 3,
        },
        ..*big
    };
    let mut rows = vec![optional];
    rows.extend(local_models().iter().copied());
    rows.leak()
}

#[test]
fn a_24_gb_gpu_gets_31b_and_never_picks_an_optional_model() {
    let p = gpu(&[24_576], 65_536);
    let picks = run(&p);
    assert_eq!(buckets(&picks), [R; 5]);
    assert_eq!(pre(&picks), Some("Gemma 4 31B"));
    assert_eq!(tier(&p), "T4");
    let picks = score(&p, with_optional_row(), &BenchCache::new());
    assert_eq!(picks[0].model.name, "Optional 30B");
    assert_eq!(picks[0].bucket, R);
    assert_eq!(pre(&picks), Some("Gemma 4 31B"));
    assert_eq!(
        why(&picks, "Optional 30B"),
        "Fits in 24 GB of GPU memory, about 40 tok/s. Optional: Loadout never picks it for you."
    );
}

#[test]
fn multi_gpu_sums_the_vram_and_is_t5() {
    let p = gpu(&[16_384, 16_384], 65_536);
    assert_eq!(p.budget(), (Memory::Gpu, 32_768));
    let picks = run(&p);
    assert_eq!(buckets(&picks), [R; 5]);
    assert_eq!(pre(&picks), Some("Gemma 4 31B"));
    assert_eq!(tier(&p), "T5");
}

#[test]
fn apple_unified_memory_budgets_70_percent_of_ram() {
    let p = apple(8_192);
    assert_eq!(p.budget(), (Memory::Unified, 5_734));
    let picks = run(&p);
    assert_eq!(buckets(&picks), [H, H, H, S, R]);
    assert_eq!(pre(&picks), Some("Gemma 4 E2B"));
    assert_eq!(tier(&p), "T1");
    assert_eq!(
        why(&picks, "Gemma 4 E2B"),
        "Fits in 5.6 GB of unified memory, about 35 tok/s."
    );
    assert_eq!(
        why(&picks, "Gemma 4 E4B"),
        "Needs 8 GB of unified memory; this PC has 5.6 GB, so it may be slow."
    );

    let picks = run(&apple(16_384));
    assert_eq!(buckets(&picks), [H, H, S, R, R]);
    assert_eq!(pre(&picks), Some("Gemma 4 E4B"));
    assert_eq!(
        why(&picks, "Gemma 4 12B"),
        "Needs 14 GB of unified memory; this PC has 11.2 GB, so it may be slow."
    );

    let picks = run(&apple(32_768));
    assert_eq!(buckets(&picks), [S, R, R, R, R]);
    assert_eq!(pre(&picks), Some("Gemma 4 26B-A4B"));
    assert_eq!(tier(&apple(32_768)), "T3");
    assert_eq!(
        why(&picks, "Gemma 4 31B"),
        "Needs 40 GB of unified memory; this PC has 22.4 GB, so it may be slow."
    );

    let picks = run(&apple(49_152));
    assert_eq!(buckets(&picks), [S, R, R, R, R]);
    assert_eq!(pre(&picks), Some("Gemma 4 26B-A4B"));

    let picks = run(&apple(65_536));
    assert_eq!(buckets(&picks), [R; 5]);
    assert_eq!(pre(&picks), Some("Gemma 4 31B"));
    assert_eq!(tier(&apple(65_536)), "T5");
}

#[test]
fn an_intel_mac_with_an_amd_gpu_scores_on_its_vram() {
    assert_eq!(pre(&run(&gpu(&[4_096], 16_384))), Some("Gemma 4 E2B"));
    assert_eq!(pre(&run(&gpu(&[8_192], 16_384))), Some("Gemma 4 E4B"));
}

#[test]
fn a_full_disk_hides_everything_with_the_space_it_needs() {
    let p = Profile {
        free_disk_mb: 4_000,
        ..gpu(&[24_576], 65_536)
    };
    let picks = run(&p);
    assert_eq!(buckets(&picks), [H; 5]);
    assert_eq!(pre(&picks), None);
    assert_eq!(tier(&p), "T0");
    assert_eq!(
        why(&picks, "Gemma 4 E2B"),
        "Needs 5.2 GB of free disk; this PC has 3.9 GB."
    );
}

#[test]
fn an_old_engine_hides_a_row_that_needs_a_newer_one_and_an_unknown_one_does_not() {
    let old = Profile {
        llama_build: Some(10_000),
        ..gpu(&[24_576], 65_536)
    };
    // No shipped row names a minimum build, so an old engine hides nothing.
    assert_eq!(buckets(&run(&old)), [R; 5]);
    let picks = score(&old, with_optional_row(), &BenchCache::new());
    assert_eq!(picks[0].bucket, H);
    assert_eq!(
        why(&picks, "Optional 30B"),
        "Needs llama.cpp b10353 or newer; this engine is b10000."
    );
    let unknown = Profile {
        llama_build: None,
        ..old
    };
    assert_eq!(
        score(&unknown, with_optional_row(), &BenchCache::new())[0].bucket,
        R
    );
    assert_eq!(build_number("b11602"), Some(11_602));
    assert_eq!(build_number("11602"), None);
}

#[test]
fn a_slow_speed_test_demotes_the_default_to_the_next_model() {
    let p = gpu(&[16_384], 32_768);
    let bench: BenchCache = [("gemma-4-26b-a4b-it-abliterated-q4_k".to_string(), 7.5)].into();
    let picks = score(&p, local_models(), &bench);
    assert_eq!(buckets(&picks), [S, S, R, R, R]);
    assert_eq!(pre(&picks), Some("Gemma 4 12B"));
    assert_eq!(
        why(&picks, "Gemma 4 26B-A4B"),
        "Measured 7.5 tok/s on this PC, so it may be slow."
    );
    let fast: BenchCache = [("gemma-4-26b-a4b-it-abliterated-q4_k".to_string(), 72.0)].into();
    assert_eq!(
        why(&score(&p, local_models(), &fast), "Gemma 4 26B-A4B"),
        "Fits in 16 GB of GPU memory, measured 72 tok/s."
    );
}

#[test]
fn budgets_from_2_gb_to_48_gb_map_to_one_tier_default() {
    let cases: [(u64, Option<&str>, &str); 9] = [
        (2_048, None, "T0"),
        (3_072, Some("Gemma 4 E2B"), "T1"),
        (6_143, Some("Gemma 4 E2B"), "T1"),
        (6_144, Some("Gemma 4 E4B"), "T2"),
        (10_240, Some("Gemma 4 12B"), "T3"),
        (15_360, Some("Gemma 4 26B-A4B"), "T3"),
        (20_480, Some("Gemma 4 31B"), "T4"),
        (24_576, Some("Gemma 4 31B"), "T4"),
        (49_152, Some("Gemma 4 31B"), "T5"),
    ];
    for (vram, want, t) in cases {
        // 4 GB of RAM leaves no CPU budget, so only the GPU counts.
        let p = gpu(&[vram], 4_096);
        assert_eq!(pre(&run(&p)), want, "{vram} MB");
        assert_eq!(tier(&p), t, "{vram} MB");
    }
}

#[test]
fn the_catalog_is_huihui_ai_abliterated_gemma_4_all_pinned() {
    let models = local_models();
    assert_eq!(models.len(), 5, "data/local_catalog.toml parses");
    assert!(models.iter().all(|m| !m.name.contains("Qwen")));
    assert!(models
        .iter()
        .all(|m| m.sha256.len() == 64 && m.sha256.bytes().all(|b| b.is_ascii_hexdigit())));
    assert!(models.iter().all(|m| m.license == "Apache-2.0"));
    assert!(models.iter().all(|m| m.quant == "Q4_K"));
    // Every row is an abliterated build, by id, repo and file.
    assert!(models.iter().all(|m| m.id.contains("-abliterated-")
        && m.repo.starts_with("huihui-ai/Huihui-gemma-4-")
        && m.repo
            .ends_with("-it-qat-q4_0-unquantized-abliterated-GGUF")
        && m.file
            .ends_with("-it-qat-q4_0-unquantized-abliterated-Q4_K.gguf")));
    assert!(models
        .iter()
        .all(|m| !m.optional && !m.is_drafter && m.min_llama_build.is_none()));
    assert_eq!(
        models
            .iter()
            .map(|m| (m.id, m.download_mb, m.run_mb))
            .collect::<Vec<_>>(),
        [
            ("gemma-4-31b-it-abliterated-q4_k", 17_821, 19_799),
            ("gemma-4-26b-a4b-it-abliterated-q4_k", 16_018, 17_400),
            ("gemma-4-12b-it-abliterated-q4_k", 7_039, 8_399),
            ("gemma-4-e4b-it-abliterated-q4_k", 5_057, 5_868),
            ("gemma-4-e2b-it-abliterated-q4_k", 3_258, 3_888),
        ]
    );
    assert_eq!(
        crate::local_setup::model_by_id("gemma-4-12b-it-abliterated-q4_k").map(|m| m.sha256),
        Some("8cfe39c96b966b2bc99d908315404914e9128fb35e134ca36f1a199507d6da6b")
    );
    // A stock Gemma 4 or a Muse Glimmer id from an older catalog finds nothing.
    for old in [
        "gemma-4-12b-it-q4_k_m",
        "gemma-4-e2b-it-q4_k_m",
        "muse-glimmer-30b-q4_k_m",
        "muse-glimmer-30b-dflash-q4_k_m",
    ] {
        assert_eq!(crate::local_setup::model_by_id(old), None, "{old}");
    }
    assert_eq!(smallest().map(|m| m.name), Some("Gemma 4 E2B"));
    assert_eq!(
        models
            .iter()
            .filter(|m| !m.needs_avx2)
            .map(|m| m.name)
            .collect::<Vec<_>>(),
        ["Gemma 4 E2B"]
    );
    // The pinned engine is new enough for every row, and its x86 builds carry
    // the AVX-only CPU variants.
    let pinned = crate::runtime_download::manifest_for_key("linux-x86_64").expect("pinned");
    assert_eq!(build_number(&pinned.version), Some(11_602));
}

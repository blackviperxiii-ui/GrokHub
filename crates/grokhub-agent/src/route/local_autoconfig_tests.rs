use super::*;
use grokhub_core::local_picker::CpuSimd;
use grokhub_core::local_profile::{CpuInfo, Gpu, GpuVendor, PowerInfo, Thermal};

fn gpu(vendor: GpuVendor, name: &str, vram_mb: u64, free: Option<u64>) -> Gpu {
    Gpu {
        vendor,
        name: name.into(),
        vram_mb,
        free_vram_mb: free,
        integrated: false,
        unified: false,
        driver: "580.95.05".into(),
    }
}

fn machine(gpus: Vec<Gpu>, ram_mb: u64, free_ram_mb: Option<u64>, cpu: CpuInfo) -> LocalProfile {
    LocalProfile {
        os: "linux",
        arch: "x86_64",
        gpus,
        cpu,
        ram_mb,
        free_ram_mb,
        power: PowerInfo::default(),
        thermal: Thermal::Nominal,
        free_disk_mb: 500_000,
    }
}

fn rtx_4070_laptop() -> LocalProfile {
    machine(
        vec![gpu(
            GpuVendor::Nvidia,
            "NVIDIA GeForce RTX 4070",
            12_282,
            Some(11_500),
        )],
        32_768,
        Some(24_000),
        CpuInfo::new(16, 8, 0, CpuSimd::Avx2),
    )
}

/// Gemma-4-12B-shaped: 48 layers, 8 KV heads of 256.
fn dense_12b() -> GgufMeta {
    GgufMeta {
        arch: "gemma4".into(),
        block_count: 48,
        context_length: 131_072,
        embedding_length: 3_840,
        head_count: 16,
        head_count_kv: 8,
        key_length: 256,
        value_length: 256,
        expert_count: 0,
        file_mb: 7_039,
        tokenizer: None,
        nextn_layers: 0,
        sliding_window: 0,
        swa_pattern: vec![],
        kv_heads: vec![],
        key_length_swa: 0,
        value_length_swa: 0,
    }
}

fn dense_4b() -> GgufMeta {
    GgufMeta {
        block_count: 36,
        head_count_kv: 4,
        file_mb: 5_088,
        ..dense_12b()
    }
}

fn moe_26b() -> GgufMeta {
    GgufMeta {
        block_count: 30,
        context_length: 262_144,
        expert_count: 128,
        file_mb: 16_018,
        ..dense_12b()
    }
}

fn dense_31b() -> GgufMeta {
    GgufMeta {
        block_count: 60,
        context_length: 262_144,
        head_count_kv: 16,
        file_mb: 17_821,
        ..dense_12b()
    }
}

fn amd_16gb_desktop() -> LocalProfile {
    machine(
        vec![gpu(GpuVendor::Amd, "AMD Radeon GPU", 16_304, None)],
        59_392,
        Some(40_000),
        CpuInfo::new(32, 16, 0, CpuSimd::Avx2),
    )
}

#[test]
fn kv_cache_size_follows_layers_heads_and_type() {
    assert_eq!(kv_mb(&dense_12b(), 16_384, KvType::Q8), 3_264);
    assert_eq!(kv_mb(&dense_12b(), 8_192, KvType::Q4), 864);
    assert_eq!(kv_mb(&dense_31b(), 32_768, KvType::Q8), 16_320);
}

/// [`dense_12b`] with Gemma-style sliding windows: five of every six layers
/// see only the last 1,024 tokens.
fn swa_12b() -> GgufMeta {
    GgufMeta {
        sliding_window: 1_024,
        swa_pattern: grokhub_core::gguf::every_nth_full(6, 48),
        ..dense_12b()
    }
}

#[test]
fn sliding_window_layers_cost_their_window_and_each_extra_slot_one_more() {
    // 40 window layers at 1,536 cells and 8 full layers at 32k, against
    // 6,528 MB with every layer at the full context.
    assert_eq!(kv_mb(&swa_12b(), 32_768, KvType::Q8), 1_343);
    assert_eq!(kv_mb(&dense_12b(), 32_768, KvType::Q8), 6_528);
    // Four slots in one pool: 4 x 1,024 + 512 window cells.
    assert_eq!(kv_mb_for(&swa_12b(), 32_768, KvType::Q8, 4), 1_853);
    // Full layers don't grow with slots.
    assert_eq!(kv_mb_for(&dense_12b(), 32_768, KvType::Q8, 4), 6_528);
    assert_eq!(swa_cells(1_024, 1, 32_768), 1_536);
    assert_eq!(swa_cells(1_000, 1, 32_768), 1_536);
    assert_eq!(swa_cells(1_024, 4, 32_768), 4_608);
    assert_eq!(swa_cells(1_024, 1, 1_024), 1_024);
    assert_eq!(swa_cells(0, 2, 8_192), 512);
}

#[test]
fn a_sliding_window_12b_on_a_12_gb_gpu_gets_twice_the_context_at_q8_and_two_slots() {
    let a = autoconfig(
        &rtx_4070_laptop(),
        &swa_12b(),
        Backend::Cuda,
        GovState::Normal,
        &Overrides::default(),
    );
    assert_eq!(
        (a.ngl, a.ctx, a.kv, a.run.seqs()),
        (49, 16_384, KvType::Q8, 2)
    );
    assert_eq!(a.run.slots.kv_unified, Some(true));
    // The same card with every layer at the full context stops at 8k.
    let dense = autoconfig(
        &rtx_4070_laptop(),
        &dense_12b(),
        Backend::Cuda,
        GovState::Normal,
        &Overrides::default(),
    );
    assert_eq!((dense.ctx, dense.run.seqs()), (8_192, 1));
}

#[test]
fn a_12_gb_gpu_keeps_the_whole_12b_on_the_gpu_with_q4_kv() {
    let a = autoconfig(
        &rtx_4070_laptop(),
        &dense_12b(),
        Backend::Cuda,
        GovState::Normal,
        &Overrides::default(),
    );
    assert_eq!(
        a,
        LaunchArgs {
            backend: Backend::Cuda,
            ngl: 49,
            all_layers: 49,
            n_cpu_moe: None,
            ctx: 8_192,
            kv: KvType::Q4,
            threads: 4,
            threads_batch: None,
            flash_attn: true,
            mlock: true,
            tensor_split: None,
            draft: None,
            cpu_only: false,
            run: run_with_slots(1),
        }
    );
    assert_eq!(
        a.to_args().join(" "),
        "-ngl 49 -c 8192 --threads 4 --cache-type-k q4_0 --cache-type-v q4_0 --flash-attn on --load-mode mmap+mlock --parallel 1 --no-kv-unified --cache-prompt"
    );
}

#[test]
fn throttle_halves_the_budget_and_offloads_part_of_the_layers() {
    let a = autoconfig(
        &rtx_4070_laptop(),
        &dense_12b(),
        Backend::Cuda,
        GovState::Throttle,
        &Overrides::default(),
    );
    assert_eq!((a.ngl, a.ctx, a.kv, a.threads), (24, 8_192, KvType::Q8, 2));
}

#[test]
fn cpu_only_runs_no_gpu_layers_and_keeps_context_short() {
    let p = machine(vec![], 16_384, None, CpuInfo::new(8, 4, 0, CpuSimd::Avx2));
    let a = autoconfig(
        &p,
        &dense_4b(),
        Backend::Cpu,
        GovState::Normal,
        &Overrides::default(),
    );
    assert_eq!(
        (a.ngl, a.ctx, a.kv, a.threads, a.flash_attn, a.mlock),
        (0, 8_192, KvType::Q8, 2, true, false)
    );
    // A q8_0 V cache needs flash attention on, or llama-server won't load.
    assert!(a.to_args().windows(2).any(|w| w == ["--flash-attn", "on"]));
}

#[test]
fn moe_on_a_16_gb_gpu_keeps_every_layer_and_moves_experts_to_ram() {
    let p = amd_16gb_desktop();
    let a = autoconfig(
        &p,
        &moe_26b(),
        Backend::Rocm,
        GovState::Normal,
        &Overrides::default(),
    );
    assert_eq!(
        (a.ngl, a.n_cpu_moe, a.ctx, a.kv, a.threads),
        (31, Some(14), 16_384, KvType::Q8, 8)
    );
    assert_eq!(
        a.to_args().join(" "),
        "-ngl 31 -c 16384 --threads 8 --cache-type-k q8_0 --cache-type-v q8_0 --flash-attn on --n-cpu-moe 14 --load-mode mmap+mlock --parallel 2 --kv-unified --cache-prompt"
    );
    // Idle: 90% of memory, every P-core, all threads for prompt batches.
    let r = autoconfig(
        &p,
        &moe_26b(),
        Backend::Rocm,
        GovState::RampUp,
        &Overrides::default(),
    );
    assert_eq!(
        (r.n_cpu_moe, r.threads, r.threads_batch),
        (Some(10), 16, Some(32))
    );
}

#[test]
fn apple_unified_memory_offloads_every_layer_when_it_fits() {
    let mac = LocalProfile {
        os: "macos",
        arch: "aarch64",
        gpus: vec![Gpu {
            unified: true,
            ..gpu(GpuVendor::Apple, "Apple M3 Pro", 22_937, None)
        }],
        ..machine(
            vec![],
            32_768,
            Some(20_000),
            CpuInfo::new(12, 12, 6, CpuSimd::Neon),
        )
    };
    let a = autoconfig(
        &mac,
        &moe_26b(),
        Backend::Metal,
        GovState::Normal,
        &Overrides::default(),
    );
    assert_eq!(
        (a.ngl, a.n_cpu_moe, a.ctx, a.kv, a.threads),
        (31, None, 8_192, KvType::Q8, 3)
    );
}

#[test]
fn two_gpus_split_the_31b_by_vram() {
    let p = machine(
        vec![
            gpu(GpuVendor::Nvidia, "RTX 4090", 24_576, None),
            gpu(GpuVendor::Nvidia, "RTX 4090", 24_576, None),
        ],
        131_072,
        Some(100_000),
        CpuInfo::new(32, 16, 0, CpuSimd::Avx2),
    );
    let a = autoconfig(
        &p,
        &dense_31b(),
        Backend::Cuda,
        GovState::Normal,
        &Overrides::default(),
    );
    assert_eq!((a.ngl, a.ctx, a.kv), (61, 32_768, KvType::Q8));
    let args = a.to_args().join(" ");
    assert!(
        args.contains("--split-mode layer --tensor-split 24576,24576"),
        "{args}"
    );
}

#[test]
fn overrides_are_clamped_to_the_model_and_the_cpu() {
    let a = autoconfig(
        &rtx_4070_laptop(),
        &dense_12b(),
        Backend::Cuda,
        GovState::Normal,
        &Overrides {
            ngl: Some(200),
            ctx: Some(4_096),
            threads: Some(99),
            kv: None,
        },
    );
    assert_eq!((a.ngl, a.ctx, a.kv, a.threads), (49, 4_096, KvType::Q8, 16));
}

#[test]
fn out_of_memory_backs_off_three_times_then_asks_for_a_smaller_model() {
    let a = autoconfig(
        &rtx_4070_laptop(),
        &dense_12b(),
        Backend::Cuda,
        GovState::Normal,
        &Overrides::default(),
    );
    let mut ngls = Vec::new();
    let mut cur = a.clone();
    for attempt in 1..=3 {
        match after_oom(&cur, attempt) {
            Retry::Again(next) => {
                ngls.push(next.ngl);
                cur = next;
            }
            Retry::SmallerModel => panic!("gave up at attempt {attempt}"),
        }
    }
    assert_eq!(ngls, [44, 39, 35]);
    assert_eq!(after_oom(&cur, 4), Retry::SmallerModel);
    let cpu = LaunchArgs { ngl: 0, ..cur };
    assert_eq!(after_oom(&cpu, 1), Retry::SmallerModel);
    // A launch with a drafter drops it before giving up any layers.
    let drafted = LaunchArgs {
        draft: Some(Box::new(DraftArgs {
            kind: grokhub_core::local_draft::DraftKind::Simple,
            model: Some(PathBuf::from("drafters/e2b.gguf")),
            on_gpu: true,
            flags: Default::default(),
        })),
        ..a.clone()
    };
    match after_oom(&drafted, 1) {
        Retry::Again(next) => assert_eq!((next.ngl, next.draft), (a.ngl, None)),
        Retry::SmallerModel => panic!("dropped the model, not the drafter"),
    }
    let cpu_drafted = LaunchArgs { ngl: 0, ..drafted };
    match after_oom(&cpu_drafted, 2) {
        Retry::Again(next) => assert_eq!((next.ngl, next.draft), (0, None)),
        Retry::SmallerModel => panic!("a CPU launch keeps trying without its drafter"),
    }
    assert_eq!(after_oom(&cpu_drafted, 4), Retry::SmallerModel);

    let moe = autoconfig(
        &amd_16gb_desktop(),
        &moe_26b(),
        Backend::Rocm,
        GovState::Normal,
        &Overrides::default(),
    );
    match after_oom(&moe, 1) {
        Retry::Again(next) => assert_eq!((next.ngl, next.n_cpu_moe), (31, Some(17))),
        Retry::SmallerModel => panic!("MoE gave up"),
    }

    assert!(oom_in_log("ggml_backend_cuda_buffer_type_alloc_buffer: allocating 9000 MiB on device 0: cudaMalloc failed: out of memory"));
    assert!(oom_in_log("ggml_vulkan: Device memory allocation of size 1 failed. vk::Device::allocateMemory: ErrorOutOfDeviceMemory"));
    assert!(!oom_in_log(
        "main: server is listening on http://127.0.0.1:8080"
    ));

    assert_eq!(
        next_smaller("gemma-4-26b-a4b-it-abliterated-q4_k").map(|m| m.name),
        Some("Gemma 4 12B")
    );
    assert_eq!(
        next_smaller("gemma-4-31b-it-abliterated-q4_k").map(|m| m.name),
        Some("Gemma 4 26B-A4B")
    );
    assert_eq!(next_smaller("gemma-4-e2b-it-abliterated-q4_k"), None);
    assert_eq!(
        catalog_model_for(Path::new(
            "/m/Huihui-gemma-4-E4B-it-qat-q4_0-unquantized-abliterated-Q4_K.gguf"
        ))
        .map(|m| m.id),
        Some("gemma-4-e4b-it-abliterated-q4_k")
    );
    // A stock Gemma 4 file from the older catalog runs as an unknown model.
    assert_eq!(
        catalog_model_for(Path::new("/m/gemma-4-E4B-it-Q4_K_M.gguf")),
        None
    );
    assert_eq!(next_smaller("gemma-4-31b-it-q4_k_m"), None);
}

#[test]
fn autotune_records_cap_the_next_plan_per_model_device_and_driver() {
    let dir = std::env::temp_dir().join(format!("grokhub-autotune-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let p = rtx_4070_laptop();
    let key = tune_key(
        Path::new("/m/gemma-4-12B-it-Q4_K_M.gguf"),
        7_381_382_944,
        &p,
        Backend::Cuda,
    );
    assert_eq!(
        key,
        "gemma-4-12B-it-Q4_K_M.gguf:7381382944|NVIDIA GeForce RTX 4070|CUDA|580.95.05"
    );
    assert_eq!(
        tune_key(Path::new("/m/x.gguf"), 1, &p, Backend::Cpu),
        "x.gguf:1|cpu|CPU|580.95.05"
    );
    assert!(load_tuned(&dir).is_empty());
    let t = Tuned {
        ngl: 39,
        n_cpu_moe: None,
        ctx: 8_192,
        kv: KvType::Q4,
        cpu_only: false,
        clean: 0,
    };
    save_tuned(&dir, &key, &t).unwrap();
    assert_eq!(load_tuned(&dir).get(&key), Some(&t));
    let fresh = autoconfig(
        &p,
        &dense_12b(),
        Backend::Cuda,
        GovState::RampUp,
        &Overrides::default(),
    );
    let capped = apply_tuned(fresh.clone(), &t);
    assert_eq!((capped.ngl, capped.ctx, capped.kv), (39, 8_192, KvType::Q4));
    assert_eq!(capped.threads, fresh.threads);
    let _ = std::fs::remove_dir_all(dir);
}

fn tiny_gguf() -> Vec<u8> {
    let mut kv = Vec::new();
    let mut put_key = |k: &str, t: u32| {
        kv.extend((k.len() as u64).to_le_bytes());
        kv.extend(k.as_bytes());
        kv.extend(t.to_le_bytes());
    };
    put_key("general.architecture", 8);
    kv.extend(5u64.to_le_bytes());
    kv.extend(b"llama");
    for (k, v) in [
        ("llama.block_count", 16u32),
        ("llama.context_length", 8_192),
        ("llama.embedding_length", 2_048),
        ("llama.attention.head_count", 32),
        ("llama.attention.head_count_kv", 8),
    ] {
        kv.extend((k.len() as u64).to_le_bytes());
        kv.extend(k.as_bytes());
        kv.extend(4u32.to_le_bytes());
        kv.extend(v.to_le_bytes());
    }
    let mut out = b"GGUF".to_vec();
    out.extend(3u32.to_le_bytes());
    out.extend(0u64.to_le_bytes());
    out.extend(6u64.to_le_bytes());
    out.extend(kv);
    out
}

#[test]
fn launch_for_reads_the_header_the_engine_and_the_stored_profile() {
    let _one = LIVE_TESTS.lock().unwrap_or_else(|e| e.into_inner());
    let root = std::env::temp_dir().join(format!("grokhub-launch-for-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("bin")).unwrap();
    let binary = root.join("bin").join("llama-server");
    std::fs::write(&binary, b"").unwrap();
    std::fs::write(root.join("bin").join("libggml-cuda.so"), b"").unwrap();
    let model = root.join("tiny.gguf");
    std::fs::write(&model, tiny_gguf()).unwrap();

    assert_eq!(launch_for(&binary, &model), None, "no hardware read yet");
    set_profile(rtx_4070_laptop(), root.join("local"));
    let (args, key, dir) = launch_for(&binary, &model).expect("planned");
    assert_eq!(args.backend, Backend::Cuda);
    assert_eq!((args.ngl, args.ctx, args.kv), (17, 8_192, KvType::Q8));
    assert!(key.starts_with("tiny.gguf:"), "{key}");
    assert_eq!(dir, root.join("local"));
    // A file that isn't GGUF falls back to the one-shot defaults.
    std::fs::write(&model, b"not gguf").unwrap();
    assert_eq!(launch_for(&binary, &model), None);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn an_igpu_only_machine_offloads_into_shared_ram_on_vulkan() {
    let igpu = Gpu {
        integrated: true,
        ..gpu(GpuVendor::Intel, "Intel iGPU", 0, None)
    };
    let p = machine(
        vec![igpu],
        32_768,
        Some(24_000),
        CpuInfo::new(12, 6, 0, CpuSimd::Avx2),
    );
    assert_eq!(
        budgets(&p, Backend::Vulkan, GovState::Normal),
        (9_600, 9_600)
    );
    assert_eq!(budgets(&p, Backend::Cpu, GovState::Normal), (0, 19_200));
    let a = autoconfig(
        &p,
        &dense_4b(),
        Backend::Vulkan,
        GovState::Normal,
        &Overrides::default(),
    );
    assert_eq!(a.ngl, 37, "every layer fits in the iGPU's half");
    // No GPU at all on a GPU backend still runs on the CPU.
    let bare = machine(
        vec![],
        32_768,
        Some(24_000),
        CpuInfo::new(12, 6, 0, CpuSimd::Avx2),
    );
    assert_eq!(
        budgets(&bare, Backend::Vulkan, GovState::Normal),
        (0, 19_200)
    );
}

#[test]
fn a_remembered_cpu_fallback_keeps_the_next_start_off_the_gpu() {
    let dir = std::env::temp_dir().join(format!("gh-tune-cpu-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let gpu = autoconfig(
        &rtx_4070_laptop(),
        &dense_12b(),
        Backend::Cuda,
        GovState::Normal,
        &Overrides::default(),
    );
    assert!(gpu.ngl > 0 && !gpu.cpu_only);
    let t = Tuned {
        ngl: 0,
        n_cpu_moe: None,
        ctx: gpu.ctx,
        kv: gpu.kv,
        cpu_only: true,
        clean: 0,
    };
    save_tuned(&dir, "k", &t).unwrap();
    let next = apply_tuned(gpu, load_tuned(&dir).get("k").unwrap());
    assert!(next.cpu_only);
    assert_eq!((next.ngl, next.backend), (0, Backend::Cpu));
    // Records written before this field read as GPU launches.
    let old: Tuned = serde_json::from_str(r#"{"ngl":39,"ctx":8192,"kv":"q4"}"#).unwrap();
    assert!(!old.cpu_only);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_launch_is_sized_to_the_memory_a_game_leaves_free_and_grows_back_after() {
    let p = rtx_4070_laptop();
    // Nothing read: the profile stands.
    assert_eq!(with_free_now(p.clone(), FreeNow::default()), p);
    // A game holds most of the card: the plan sees 3 GB free, not the 11.5 GB
    // the setup probe saw, and RAM follows the live read too.
    let game = with_free_now(
        p.clone(),
        FreeNow {
            vram_free_mb: Some(3_000),
            ram_free_mb: Some(10_000),
        },
    );
    assert_eq!(game.gpus[0].free_vram_mb, Some(3_000));
    assert_eq!(game.free_ram_mb, Some(10_000));
    let (gpu_before, _) = budgets(&p, Backend::Cuda, GovState::Throttle);
    let (gpu_game, ram_game) = budgets(&game, Backend::Cuda, GovState::Throttle);
    assert_eq!((gpu_before, gpu_game, ram_game), (5_750, 1_500, 5_000));
    // Fewer layers on the GPU and a short context, instead of an OOM start.
    let full = autoconfig(
        &p,
        &dense_12b(),
        Backend::Cuda,
        GovState::Normal,
        &Overrides::default(),
    );
    let shared = autoconfig(
        &game,
        &dense_12b(),
        Backend::Cuda,
        GovState::Throttle,
        &Overrides::default(),
    );
    assert_eq!(full.ngl, full.all_layers);
    assert!(shared.ngl < shared.all_layers, "{shared:?}");
    assert!(shared.ctx <= 8_192 && shared.ctx >= 4_096, "{shared:?}");
    // The game took much more than the launch planned for: relaunch; a small
    // wobble does not.
    assert!(vram_dropped(11_500, 3_000));
    assert!(!vram_dropped(11_500, 9_000));
    assert!(!vram_dropped(0, 0));
    // Two cards share the live read in proportion to their size.
    let two = machine(
        vec![
            gpu(GpuVendor::Nvidia, "A", 24_000, Some(20_000)),
            gpu(GpuVendor::Nvidia, "B", 12_000, Some(10_000)),
        ],
        65_536,
        Some(40_000),
        CpuInfo::new(32, 16, 0, CpuSimd::Avx2),
    );
    let two = with_free_now(
        two,
        FreeNow {
            vram_free_mb: Some(9_000),
            ram_free_mb: None,
        },
    );
    assert_eq!(
        (
            two.gpus[0].free_vram_mb,
            two.gpus[1].free_vram_mb,
            two.free_ram_mb
        ),
        (Some(6_000), Some(3_000), Some(40_000))
    );
}

#[test]
fn the_vram_baseline_is_taken_with_our_model_loaded_so_loading_it_never_reads_as_a_game() {
    use std::sync::atomic::{AtomicU64, Ordering};
    static FREE: AtomicU64 = AtomicU64::new(0);
    set_free_probe(Arc::new(|| FreeNow {
        vram_free_mb: Some(FREE.load(Ordering::SeqCst)),
        ram_free_mb: None,
    }));
    // 7.5 GB free before launch; planning never sets the baseline.
    FREE.store(7_500, Ordering::SeqCst);
    if let Ok(mut l) = live().lock() {
        l.launch_free_vram_mb = None;
    }
    assert!(
        !vram_dropped_since_launch(),
        "no baseline before the model is resident"
    );
    // The model took 3.3 GB: that is the baseline, not a drop.
    FREE.store(4_200, Ordering::SeqCst);
    mark_resident();
    assert!(!vram_dropped_since_launch());
    FREE.store(3_500, Ordering::SeqCst);
    assert!(!vram_dropped_since_launch(), "a small dip stays");
    // A game took most of what was left.
    FREE.store(1_200, Ordering::SeqCst);
    assert!(vram_dropped_since_launch());
    // A new launch starts: the old model's baseline no longer counts.
    clear_resident();
    assert!(!vram_dropped_since_launch());
    reset_free_probe();
}

fn whole_gpu_12b() -> LaunchArgs {
    LaunchArgs {
        backend: Backend::Vulkan,
        ngl: 49,
        all_layers: 49,
        n_cpu_moe: None,
        ctx: 16_384,
        kv: KvType::Q8,
        threads: 8,
        threads_batch: Some(16),
        flash_attn: true,
        mlock: false,
        tensor_split: None,
        draft: None,
        cpu_only: false,
        run: Default::default(),
    }
}

#[test]
fn a_whole_gpu_launch_that_only_changes_threads_is_the_same_launch() {
    let running = whole_gpu_12b();
    assert!(same_launch(&running, &running.clone()));
    let fewer_threads = LaunchArgs {
        threads: 4,
        threads_batch: None,
        ..whole_gpu_12b()
    };
    assert!(same_launch(&running, &fewer_threads));
    let shorter = LaunchArgs {
        ctx: 8_192,
        ..whole_gpu_12b()
    };
    assert!(!same_launch(&running, &shorter));
    // Part of it on the CPU: threads do the work there, so they matter.
    let partial = LaunchArgs {
        ngl: 30,
        ..whole_gpu_12b()
    };
    let partial_fewer = LaunchArgs {
        threads: 4,
        ..partial.clone()
    };
    assert!(!same_launch(&partial, &partial_fewer));
}

#[test]
fn a_launch_needs_its_gpu_weights_kv_and_buffers_on_the_gpu_and_the_rest_in_ram() {
    assert_eq!(
        launch_need_mb(&dense_12b(), &whole_gpu_12b()),
        (10_943, 512)
    );
    let moe = LaunchArgs {
        ngl: 31,
        all_layers: 31,
        n_cpu_moe: Some(10),
        ctx: 8_192,
        ..whole_gpu_12b()
    };
    assert_eq!(launch_need_mb(&moe_26b(), &moe), (13_342, 4_784));
    let cpu = on_cpu(&whole_gpu_12b());
    assert_eq!(launch_need_mb(&dense_12b(), &cpu), (0, 10_943));
}

#[test]
fn a_second_server_fits_beside_only_with_a_margin_and_a_known_reading() {
    let free = |vram: Option<u64>, ram: Option<u64>| FreeNow {
        vram_free_mb: vram,
        ram_free_mb: ram,
    };
    assert!(fits_beside(free(Some(12_100), Some(3_000)), (11_000, 512)));
    assert!(!fits_beside(free(Some(12_099), Some(3_000)), (11_000, 512)));
    assert!(!fits_beside(free(None, Some(3_000)), (11_000, 512)));
    assert!(!fits_beside(free(Some(20_000), Some(2_600)), (11_000, 512)));
    assert!(fits_beside(free(None, Some(14_100)), (0, 10_943)));
    assert!(!fits_beside(free(None, Some(14_000)), (0, 10_943)));
}

#[test]
fn the_running_models_own_memory_is_added_back_for_a_fresh_plan() {
    let before = FreeNow {
        vram_free_mb: Some(8_000),
        ram_free_mb: Some(30_000),
    };
    let resident = FreeNow {
        vram_free_mb: Some(4_000),
        ram_free_mb: Some(29_000),
    };
    let own = taken(before, resident);
    assert_eq!(
        own,
        FreeNow {
            vram_free_mb: Some(4_000),
            ram_free_mb: Some(1_000),
        }
    );
    let now = FreeNow {
        vram_free_mb: Some(3_000),
        ram_free_mb: None,
    };
    assert_eq!(
        with_own(now, own),
        FreeNow {
            vram_free_mb: Some(7_000),
            ram_free_mb: None,
        }
    );
    assert_eq!(taken(FreeNow::default(), resident), FreeNow::default());
}

/// Tests that set the global profile run one at a time.
static LIVE_TESTS: Mutex<()> = Mutex::new(());

#[cfg(unix)]
#[test]
fn a_model_kept_up_under_throttle_gets_the_sharing_flags_its_engine_knows() {
    use std::os::unix::fs::PermissionsExt;
    let _one = LIVE_TESTS.lock().unwrap_or_else(|e| e.into_inner());
    let root = std::env::temp_dir().join(format!("grokhub-throttle-flags-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("bin")).unwrap();
    let binary = root.join("bin").join("llama-server");
    std::fs::write(
        &binary,
        "#!/bin/sh\necho '--prio N   priority'\necho '--poll <0...100>   polling'\necho '-ub,   --ubatch-size N   ubatch'\n",
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(root.join("bin").join("libggml-cuda.so"), b"").unwrap();
    let model = root.join("tiny.gguf");
    std::fs::write(&model, tiny_gguf()).unwrap();
    set_profile(rtx_4070_laptop(), root.join("local"));
    set_state(GovState::Normal);
    let (normal, _, _) = launch_for(&binary, &model).expect("planned");
    // Normal is tuned (one slot at 8k, no drafter); Throttle never is.
    let tag = normal.run.tune.clone().expect("tuned");
    assert_eq!(tag.arm.label(), "np1-plain-ub512");
    assert_eq!(
        normal.run,
        super::super::local_flags::RunFlags {
            tune: Some(tag),
            ..Default::default()
        }
    );
    set_state(GovState::Throttle);
    let (shared, _, _) = launch_for(&binary, &model).expect("planned");
    set_state(GovState::Normal);
    assert_eq!(shared.run.poll, Some(0));
    assert_eq!(shared.run.prio, Some(-1));
    assert_eq!(shared.run.ubatch, Some(128));
    assert_eq!(shared.run.vk_nodes_per_submit, None, "CUDA, not Vulkan");
    assert!(shared.run.low_priority);
    assert_eq!(shared.run.tune, None);
    let args = shared.to_args();
    assert!(args.windows(2).any(|w| w == ["--poll", "0"]));
    assert!(args.windows(2).any(|w| w == ["--prio", "-1"]));
    assert!(args.windows(2).any(|w| w == ["--ubatch-size", "128"]));
    // Later tests start with no hardware read.
    if let Ok(mut l) = live().lock() {
        l.profile = None;
        l.tune_dir = None;
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn an_out_of_memory_cap_grows_back_a_step_after_two_clean_starts() {
    let dir = crate::harness::test_dir("autotune-grow");
    let capped = Tuned {
        ngl: 30,
        n_cpu_moe: Some(12),
        ctx: 16_384,
        kv: KvType::Q8,
        cpu_only: false,
        clean: 0,
    };
    save_tuned(&dir, "k", &capped).unwrap();
    clean_start(&dir, "k", 31).unwrap();
    assert_eq!(load_tuned(&dir).get("k").map(|t| t.clean), Some(1));
    clean_start(&dir, "k", 31).unwrap();
    assert_eq!(
        load_tuned(&dir).get("k"),
        Some(&Tuned {
            ngl: 31,
            n_cpu_moe: Some(9),
            ctx: 32_768,
            kv: KvType::Q8,
            cpu_only: false,
            clean: 0,
        })
    );
    // Once nothing is capped any more, the record goes.
    let near = Tuned {
        ngl: 31,
        n_cpu_moe: None,
        ctx: 65_536,
        kv: KvType::Q8,
        cpu_only: false,
        clean: 1,
    };
    save_tuned(&dir, "k", &near).unwrap();
    clean_start(&dir, "k", 31).unwrap();
    assert_eq!(load_tuned(&dir).get("k"), None);
    // A GPU crash record stays: a clean CPU start says nothing about the GPU.
    let crashed = Tuned {
        cpu_only: true,
        ..capped
    };
    save_tuned(&dir, "c", &crashed).unwrap();
    clean_start(&dir, "c", 31).unwrap();
    clean_start(&dir, "c", 31).unwrap();
    assert_eq!(load_tuned(&dir).get("c"), Some(&crashed));
}

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
        }
    );
    assert_eq!(
        a.to_args().join(" "),
        "-ngl 49 -c 8192 --threads 4 --cache-type-k q4_0 --cache-type-v q4_0 --flash-attn on --load-mode mmap+mlock --cache-prompt"
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
        "-ngl 31 -c 16384 --threads 8 --cache-type-k q8_0 --cache-type-v q8_0 --flash-attn on --n-cpu-moe 14 --load-mode mmap+mlock --cache-prompt"
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
}

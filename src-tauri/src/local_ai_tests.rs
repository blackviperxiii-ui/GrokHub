use super::*;

fn gpu(name: &str, vram_mb: u64) -> Gpu {
    Gpu {
        vendor: lp::GpuVendor::Nvidia,
        name: name.into(),
        vram_mb,
        free_vram_mb: None,
        integrated: false,
        unified: false,
        driver: String::new(),
    }
}

fn machine(gpus: Vec<Gpu>, ram_mb: u64) -> Machine {
    Machine {
        reader: if gpus.is_empty() {
            GpuReader::None
        } else {
            GpuReader::NvidiaSmi
        },
        gpus,
        unified: false,
        ram_mb: Some(ram_mb),
        free_disk_mb: Some(500_000),
        simd: CpuSimd::Avx2,
        llama_build: Some(11_602),
    }
}

fn apple(ram_mb: u64) -> Machine {
    Machine {
        unified: true,
        simd: CpuSimd::Neon,
        ..machine(Vec::new(), ram_mb)
    }
}

/// (name, bucket) in page order, the pre-selected id, and the reason of one model.
fn page(m: &Machine) -> (Vec<(String, &'static str)>, String, Picker) {
    let p = m.profile();
    let picker = picker(&p, &BenchCache::new(), &[], &LocalAiConfig::default());
    let order = picker
        .models
        .iter()
        .map(|r| (r.name.clone(), r.bucket))
        .collect();
    (order, picker.selected.clone(), picker)
}

fn why(picker: &Picker, name: &str) -> String {
    picker
        .models
        .iter()
        .find(|r| r.name == name)
        .map(|r| r.reason.clone())
        .unwrap_or_default()
}

fn named<'a>(order: &'a [(String, &'static str)]) -> Vec<(&'a str, &'static str)> {
    order.iter().map(|(n, b)| (n.as_str(), *b)).collect()
}

#[test]
fn a_t1_laptop_preselects_e2b_and_hides_what_cant_run() {
    let (order, selected, picker) = page(&machine(Vec::new(), 8_192));
    assert_eq!(
        named(&order),
        [
            ("Gemma 4 E2B", "recommended"),
            ("Gemma 4 E4B", "maySlow"),
            ("Gemma 4 31B", "hidden"),
            ("Gemma 4 26B-A4B", "hidden"),
            ("Gemma 4 12B", "hidden"),
        ]
    );
    assert_eq!(selected, "gemma-4-e2b-it-abliterated-q4_k");
    assert_eq!(
        why(&picker, "Gemma 4 E4B"),
        "Needs 6 GB of spare RAM; this PC has 4 GB, so it may be slow."
    );
    assert_eq!(
        why(&picker, "Gemma 4 12B"),
        "Needs 8.2 GB of memory; this PC has 6 GB."
    );
    let e2b = &picker.models[0];
    assert_eq!(
        (e2b.size.as_str(), e2b.quant.as_str(), e2b.tok_s.as_str()),
        ("3.2 GB", "Q4_K", "about 7 tok/s")
    );
}

#[test]
fn an_8_gb_gpu_preselects_e4b() {
    let (order, selected, picker) = page(&machine(vec![gpu("RTX 4060", 8_192)], 16_384));
    assert_eq!(
        named(&order),
        [
            ("Gemma 4 E4B", "recommended"),
            ("Gemma 4 E2B", "recommended"),
            ("Gemma 4 31B", "maySlow"),
            ("Gemma 4 26B-A4B", "maySlow"),
            ("Gemma 4 12B", "maySlow"),
        ]
    );
    assert_eq!(selected, "gemma-4-e4b-it-abliterated-q4_k");
    assert_eq!(
        why(&picker, "Gemma 4 12B"),
        "Needs 10 GB of GPU memory; this PC has 8 GB, so part of it runs on the CPU and it may be slow."
    );
}

#[test]
fn a_16_gb_gpu_shows_the_31b_as_slow_with_partial_offload() {
    let (order, selected, picker) = page(&machine(vec![gpu("RX 9070 XT", 16_304)], 59_392));
    assert_eq!(
        named(&order),
        [
            ("Gemma 4 26B-A4B", "recommended"),
            ("Gemma 4 12B", "recommended"),
            ("Gemma 4 E4B", "recommended"),
            ("Gemma 4 E2B", "recommended"),
            ("Gemma 4 31B", "maySlow"),
        ]
    );
    assert_eq!(selected, "gemma-4-26b-a4b-it-abliterated-q4_k");
    assert_eq!(
        why(&picker, "Gemma 4 31B"),
        "Needs 20 GB of GPU memory; this PC has 15.9 GB, so part of it runs on the CPU and it may be slow."
    );
}

#[test]
fn a_24_gb_gpu_recommends_every_model_and_preselects_31b() {
    let (order, selected, picker) = page(&machine(vec![gpu("RTX 4090", 24_576)], 65_536));
    assert!(order.iter().all(|(_, b)| *b == "recommended"), "{order:?}");
    assert_eq!(selected, "gemma-4-31b-it-abliterated-q4_k");
    assert_eq!(
        named(&order),
        [
            ("Gemma 4 31B", "recommended"),
            ("Gemma 4 26B-A4B", "recommended"),
            ("Gemma 4 12B", "recommended"),
            ("Gemma 4 E4B", "recommended"),
            ("Gemma 4 E2B", "recommended"),
        ]
    );
    assert!(picker.models.iter().all(|r| !r.optional));
    assert_eq!(
        why(&picker, "Gemma 4 31B"),
        "Fits in 24 GB of GPU memory, about 35 tok/s."
    );
}

#[test]
fn an_apple_16_gb_mac_preselects_e4b_on_unified_memory() {
    let m = apple(16_384);
    let (order, selected, picker) = page(&m);
    assert_eq!(
        named(&order),
        [
            ("Gemma 4 E4B", "recommended"),
            ("Gemma 4 E2B", "recommended"),
            ("Gemma 4 12B", "maySlow"),
            ("Gemma 4 31B", "hidden"),
            ("Gemma 4 26B-A4B", "hidden"),
        ]
    );
    assert_eq!(selected, "gemma-4-e4b-it-abliterated-q4_k");
    assert_eq!(
        why(&picker, "Gemma 4 12B"),
        "Needs 14 GB of unified memory; this PC has 11.2 GB, so it may be slow."
    );
    let s = status(
        &m.profile(),
        &picker,
        &BenchCache::new(),
        true,
        Level::Normal,
        "normal use",
    );
    assert_eq!(
        s,
        Status {
            tier: "T2".into(),
            model: "Gemma 4 E4B".into(),
            state: "Normal".into(),
            reason: "normal use".into(),
            tok_s: "about 30 tok/s".into(),
            memory: "11.2 GB of unified memory".into(),
        }
    );
    assert_eq!(tray_label(&s), "Local AI: Normal");
}

#[test]
fn a_saved_pick_wins_and_status_says_when_nothing_is_set_up() {
    let m = machine(vec![gpu("RTX 4090", 24_576)], 65_536);
    let config = LocalAiConfig {
        local_model: Selected {
            selected: "gemma-4-12b-it-abliterated-q4_k".into(),
        },
        ..LocalAiConfig::default()
    };
    let p = m.profile();
    let picker = picker(&p, &BenchCache::new(), &[], &config);
    assert_eq!(picker.selected, "gemma-4-12b-it-abliterated-q4_k");
    let s = status(&p, &picker, &BenchCache::new(), false, Level::Normal, "x");
    assert_eq!(
        (s.model.as_str(), s.state.as_str(), s.reason.as_str()),
        (
            "Gemma 4 12B",
            "Not set up",
            "No local engine or model is installed yet."
        )
    );
    // A stock Gemma 4 id saved by an older catalog is no catalog model now:
    // it shows as saved, with no speed estimate.
    let stale = super::picker(
        &p,
        &BenchCache::new(),
        &[],
        &LocalAiConfig {
            local_model: Selected {
                selected: "gemma-4-12b-it-q4_k_m".into(),
            },
            ..LocalAiConfig::default()
        },
    );
    let s = status(&p, &stale, &BenchCache::new(), true, Level::Normal, "x");
    assert_eq!(
        (s.model.as_str(), s.tok_s.as_str()),
        ("gemma-4-12b-it-q4_k_m", "")
    );
    let paused = status(
        &p,
        &picker,
        &BenchCache::new(),
        true,
        Level::Unload,
        "a game is running",
    );
    assert_eq!(tray_label(&paused), "Local AI: Paused");
}

#[test]
fn values_are_clamped_and_caps_never_pass_90_percent() {
    let wild = LocalAiConfig {
        mode: Mode::Manual,
        manual: Manual {
            backend: "Vulkan".into(),
            draft: "maybe".into(),
            ngl: Some(5_000),
            ctx: Some(100),
            threads: Some(64),
            kv: "q4".into(),
            idle_minutes: 0,
            caps: Caps {
                ramp_up: 100,
                normal: 95,
                throttle: 0,
            },
            never_unload: true,
            allow_apps: vec![" vlc.exe ".into(), "VLC.exe".into(), "".into()],
            deny_apps: vec!["blender".into()],
        },
        ..LocalAiConfig::default()
    }
    .clamped(16);
    let m = &wild.manual;
    assert_eq!(
        (m.backend.as_str(), m.draft.as_str(), m.kv.as_str()),
        ("vulkan", "auto", "q4")
    );
    assert_eq!(
        (m.ngl, m.ctx, m.threads),
        (Some(999), Some(2_048), Some(16))
    );
    assert_eq!(m.idle_minutes, 1);
    assert_eq!(
        m.caps,
        Caps {
            ramp_up: 90,
            normal: 90,
            throttle: 10
        }
    );
    assert_eq!(m.allow_apps, ["vlc.exe"]);
    assert_eq!(
        wild.overrides(),
        local_autoconfig::Overrides {
            ngl: Some(999),
            ctx: Some(2_048),
            threads: Some(16),
            kv: Some(local_autoconfig::KvType::Q4),
        }
    );
    let auto = LocalAiConfig {
        mode: Mode::Auto,
        ..wild.clone()
    };
    assert_eq!(auto.overrides(), local_autoconfig::Overrides::default());
}

#[test]
fn saving_writes_the_pick_and_the_governors_rules() {
    let dir = std::env::temp_dir().join(format!("grokhub-local-ai-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(load(&dir), LocalAiConfig::default());
    let config = LocalAiConfig {
        pause_for_games: false,
        use_idle_time: false,
        throttle_on_battery: false,
        local_model: Selected {
            selected: "/models/x.gguf".into(),
        },
        manual: Manual {
            deny_apps: vec!["blender".into()],
            ..Manual::default()
        },
        ..LocalAiConfig::default()
    };
    save(&dir, &config).unwrap();
    assert_eq!(load(&dir), config);
    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join(FILE)).unwrap()).unwrap();
    assert_eq!(raw["localModel"]["selected"], "/models/x.gguf");
    assert_eq!(raw["mode"], "auto");
    let rules = Rules::from_json(
        &std::fs::read_to_string(dir.join("local").join("governor.json")).unwrap(),
    );
    assert_eq!(
        rules,
        Rules {
            allow: Vec::new(),
            deny: vec!["blender".into()],
            throttle_on_battery: false,
            pause_for_games: false,
            use_idle_time: false,
        }
    );
    std::fs::write(dir.join(FILE), "{broken").unwrap();
    assert_eq!(load(&dir), LocalAiConfig::default());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_third_party_file_needs_the_notice_once() {
    let seen = vec!["/m/Llama-3-8B-abliterated.Q4_K_M.gguf".to_string()];
    // The catalog's own abliterated builds carry the badge but no notice.
    let catalog = file_row(
        Path::new("/m/Huihui-gemma-4-E4B-it-qat-q4_0-unquantized-abliterated-Q4_K.gguf"),
        5 * 1_024 * 1_024 * 1_024,
        "Loadout models",
        &[],
    );
    assert_eq!(
        (
            catalog.third_party,
            catalog.abliterated,
            catalog.needs_notice
        ),
        (false, true, false)
    );
    assert_eq!(catalog.size, "5 GB");
    // A stock Gemma 4 file from the older catalog is a third-party file now.
    let stock = file_row(
        Path::new("/m/gemma-4-E4B-it-Q4_K_M.gguf"),
        0,
        "LM Studio",
        &[],
    );
    assert_eq!(
        (stock.third_party, stock.abliterated, stock.needs_notice),
        (true, false, true)
    );
    let other = file_row(Path::new("/m/qwen3-8b-q4.gguf"), 0, "Ollama", &[]);
    assert_eq!(
        (other.third_party, other.abliterated, other.needs_notice),
        (true, false, true)
    );
    let ablit = file_row(
        Path::new("/m/Llama-3-8B-abliterated.Q4_K_M.gguf"),
        0,
        "Model file",
        &[],
    );
    assert_eq!((ablit.abliterated, ablit.needs_notice), (true, true));
    let read = file_row(
        Path::new("/m/Llama-3-8B-abliterated.Q4_K_M.gguf"),
        0,
        "Model file",
        &seen,
    );
    assert!(!read.needs_notice);
}

#[test]
fn files_on_this_pc_are_listed_but_never_preselected() {
    let m = machine(Vec::new(), 8_192);
    let found = [FoundModel {
        name: "uncensored-7b.gguf".into(),
        path: PathBuf::from("/m/uncensored-7b.gguf"),
        bytes: 0,
        source: Source::Ollama,
        sha256: None,
    }];
    let picker = picker(
        &m.profile(),
        &BenchCache::new(),
        &found,
        &LocalAiConfig::default(),
    );
    assert_eq!(picker.selected, "gemma-4-e2b-it-abliterated-q4_k");
    assert_eq!(picker.on_this_pc.len(), 1);
    let f = &picker.on_this_pc[0];
    assert_eq!(
        (f.name.as_str(), f.source, f.abliterated, f.needs_notice),
        ("uncensored-7b.gguf", "Ollama", true, true)
    );
}

#[test]
fn a_model_file_must_be_a_gguf_that_exists() {
    assert_eq!(
        model_file("/m/notes.txt", &[]),
        Err("Pick a .gguf model file.".into())
    );
    let missing = std::env::temp_dir().join("grokhub-no-such-model.gguf");
    assert_eq!(
        model_file(&missing.display().to_string(), &[]),
        Err(format!("{} isn't there.", missing.display()))
    );
    let dir = std::env::temp_dir().join(format!("grokhub-model-file-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("Mistral-abliterated.Q4_K_M.gguf");
    std::fs::write(&file, b"gguf").unwrap();
    let row = model_file(&format!(" {} ", file.display()), &[]).unwrap();
    assert_eq!(
        (row.source, row.third_party, row.needs_notice),
        ("Model file", true, true)
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn limited_detection_is_named() {
    let none = |_: &str| None;
    assert_eq!(
        limits(
            &machine(vec![gpu("RTX 4090", 24_576)], 65_536),
            "linux",
            &none
        ),
        Vec::<String>::new()
    );
    let blind = Machine {
        reader: GpuReader::Vulkan,
        gpus: vec![gpu("NVIDIA GeForce RTX 4060", 0)],
        ram_mb: None,
        free_disk_mb: None,
        ..machine(Vec::new(), 0)
    };
    assert_eq!(
        limits(&blind, "windows", &none),
        [
            "Vulkan found NVIDIA GeForce RTX 4060 but can't read its memory, so models are sized for RAM.",
            "RAM couldn't be read, so every model is sized as if there were none.",
            "Free disk space couldn't be read, so models aren't checked against it.",
        ]
    );
    let wayland = |k: &str| (k == "XDG_SESSION_TYPE").then(|| "wayland".to_string());
    assert_eq!(
        limits(&machine(Vec::new(), 8_192), "linux", &wayland),
        [
            "No GPU reader found (nvidia-smi or vulkaninfo), so models are sized for RAM.",
            "On Wayland Loadout can't see which app is fullscreen, so Pause while gaming only sees games it can detect.",
        ]
    );
    let hypr = |k: &str| match k {
        "XDG_SESSION_TYPE" => Some("wayland".to_string()),
        "HYPRLAND_INSTANCE_SIGNATURE" => Some("abc".to_string()),
        _ => None,
    };
    assert_eq!(limits(&apple(16_384), "macos", &none), Vec::<String>::new());
    assert_eq!(
        limits(
            &machine(vec![gpu("RTX 4090", 24_576)], 65_536),
            "linux",
            &hypr
        ),
        Vec::<String>::new()
    );
}

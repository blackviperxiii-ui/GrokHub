use std::io::{Read, Write};
use std::net::TcpListener;

use super::*;
use crate::route::local_autoconfig::{autoconfig, KvType, Overrides};
use grokhub_core::local_draft::Verdict;
use grokhub_core::local_picker::CpuSimd;
use grokhub_core::local_profile::{Backend, CpuInfo, Gpu, GpuVendor, PowerInfo, Thermal};

const B11602_HELP: &str = "\
--spec-draft-n-max N                    number of tokens to draft for speculative decoding (default: 3)
--spec-draft-ngl, -ngld, --gpu-layers-draft, --n-gpu-layers-draft N
--spec-draft-model, -md, --model-draft FNAME
--spec-type none,draft-simple,draft-eagle3,draft-mtp,draft-dflash
--draft, --draft-n, --draft-max N       the argument has been removed. use --spec-draft-n-max or
";

fn nvidia(vram_mb: u64, free: u64) -> Gpu {
    Gpu {
        vendor: GpuVendor::Nvidia,
        name: "NVIDIA GeForce RTX".into(),
        vram_mb,
        free_vram_mb: Some(free),
        integrated: false,
        unified: false,
        driver: "580.95.05".into(),
    }
}

fn machine(gpus: Vec<Gpu>, ram_mb: u64, free_ram_mb: u64) -> LocalProfile {
    LocalProfile {
        os: "linux",
        arch: "x86_64",
        gpus,
        cpu: CpuInfo::new(16, 8, 0, CpuSimd::Avx2),
        ram_mb,
        free_ram_mb: Some(free_ram_mb),
        power: PowerInfo::default(),
        thermal: Thermal::Nominal,
        free_disk_mb: 500_000,
    }
}

/// Gemma-4-12B-shaped, with a vocabulary hash.
fn gemma_12b(tokenizer: Option<String>) -> GgufMeta {
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
        tokenizer,
        nextn_layers: 0,
    }
}

const E2B_MB: u64 = 3_100;

fn args(p: &LocalProfile, meta: &GgufMeta, backend: Backend, state: GovState) -> LaunchArgs {
    autoconfig(p, meta, backend, state, &Overrides::default())
}

#[test]
fn draft_flags_go_out_in_the_engines_own_names() {
    let f = ld::parse_help(B11602_HELP);
    let simple = DraftArgs {
        kind: DraftKind::Simple,
        model: Some(PathBuf::from("/m/drafters/gemma-4-E2B-it-Q4_K_M.gguf")),
        on_gpu: true,
        flags: f.clone(),
    };
    assert_eq!(
        simple.to_args().join(" "),
        "--spec-type draft-simple --spec-draft-model /m/drafters/gemma-4-E2B-it-Q4_K_M.gguf --spec-draft-ngl all --spec-draft-n-max 16"
    );
    let dflash = DraftArgs {
        kind: DraftKind::Dflash,
        model: Some(PathBuf::from("/m/drafters/dflash.gguf")),
        on_gpu: false,
        flags: f.clone(),
    };
    assert_eq!(
        dflash.to_args().join(" "),
        "--spec-type draft-dflash --spec-draft-model /m/drafters/dflash.gguf --spec-draft-ngl 0"
    );
    let mtp = DraftArgs {
        kind: DraftKind::Mtp,
        model: None,
        on_gpu: true,
        flags: f,
    };
    assert_eq!(mtp.to_args().join(" "), "--spec-type draft-mtp");
    let old = ld::parse_help(
        "-md,   --model-draft FNAME   draft model\n-ngld, --gpu-layers-draft N\n--draft-max, --draft, --draft-n N   tokens to draft\n",
    );
    let legacy = DraftArgs {
        kind: DraftKind::Simple,
        model: Some(PathBuf::from("d.gguf")),
        on_gpu: false,
        flags: old,
    };
    assert_eq!(
        legacy.to_args().join(" "),
        "--model-draft d.gguf --gpu-layers-draft 0 --draft-max 16"
    );
    // The draft flags sit just before `--cache-prompt` in the launch.
    let p = machine(vec![nvidia(24_576, 23_000)], 65_536, 48_000);
    let mut a = args(&p, &gemma_12b(None), Backend::Cuda, GovState::Normal);
    a.draft = Some(Box::new(simple));
    let line = a.to_args().join(" ");
    assert!(
        line.ends_with("--spec-draft-ngl all --spec-draft-n-max 16 --cache-prompt"),
        "{line}"
    );
}

#[test]
fn a_drafter_fits_only_beside_a_whole_model_or_on_the_cpu() {
    let meta = gemma_12b(None);
    // 24 GB card: the 12B fits whole at 32k with room for E2B.
    let big = machine(vec![nvidia(24_576, 23_000)], 65_536, 48_000);
    let a = args(&big, &meta, Backend::Cuda, GovState::Normal);
    assert_eq!((a.ngl, a.ctx, a.kv), (49, 32_768, KvType::Q8));
    assert_eq!(fits(&big, &meta, &a, GovState::Normal, E2B_MB), Some(true));
    // Throttle halves the budget: no room for both.
    let t = args(&big, &meta, Backend::Cuda, GovState::Throttle);
    assert_eq!(fits(&big, &meta, &t, GovState::Throttle, E2B_MB), None);
    // 12 GB laptop card: the 12B alone takes it.
    let laptop = machine(vec![nvidia(12_282, 11_500)], 32_768, 24_000);
    let l = args(&laptop, &meta, Backend::Cuda, GovState::Normal);
    assert_eq!(fits(&laptop, &meta, &l, GovState::Normal, E2B_MB), None);
    // 6 GB card: part of the model runs on the CPU, so nothing to spare.
    let small = machine(vec![nvidia(6_144, 5_800)], 32_768, 24_000);
    let s = args(&small, &meta, Backend::Cuda, GovState::Normal);
    assert!(s.ngl < s.all_layers, "{s:?}");
    assert_eq!(fits(&small, &meta, &s, GovState::Normal, E2B_MB), None);
    // CPU only with 64 GB: the drafter sits in RAM too.
    let cpu = machine(vec![], 65_536, 48_000);
    let c = args(&cpu, &meta, Backend::Cpu, GovState::Normal);
    assert_eq!(fits(&cpu, &meta, &c, GovState::Normal, E2B_MB), Some(false));
    // 8 GB, CPU only: no.
    let low = machine(vec![], 8_192, 5_000);
    let lo = args(&low, &meta, Backend::Cpu, GovState::Normal);
    assert_eq!(fits(&low, &meta, &lo, GovState::Normal, E2B_MB), None);
}

/// A GGUF header with a vocabulary, written to `path`.
fn write_gguf(path: &Path, arch: &str, tokens: &[&str], nextn: u32) {
    fn s(out: &mut Vec<u8>, v: &str) {
        out.extend((v.len() as u64).to_le_bytes());
        out.extend(v.as_bytes());
    }
    let mut kv = Vec::new();
    s(&mut kv, "general.architecture");
    kv.extend(8u32.to_le_bytes());
    s(&mut kv, arch);
    s(&mut kv, "tokenizer.ggml.model");
    kv.extend(8u32.to_le_bytes());
    s(&mut kv, "gemma4");
    s(&mut kv, "tokenizer.ggml.tokens");
    kv.extend(9u32.to_le_bytes());
    kv.extend(8u32.to_le_bytes());
    kv.extend((tokens.len() as u64).to_le_bytes());
    for t in tokens {
        s(&mut kv, t);
    }
    s(&mut kv, &format!("{arch}.nextn_predict_layers"));
    kv.extend(4u32.to_le_bytes());
    kv.extend(nextn.to_le_bytes());
    let mut out = b"GGUF".to_vec();
    out.extend(3u32.to_le_bytes());
    out.extend(0u64.to_le_bytes());
    out.extend(4u64.to_le_bytes());
    out.extend(kv);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, out).unwrap();
}

const VOCAB: &[&str] = &["<pad>", "<eos>", "<bos>", "hello", "world"];

fn tmp(label: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("grokhub-spec-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

struct Rig {
    dir: PathBuf,
    model: PathBuf,
    meta: GgufMeta,
    p: LocalProfile,
    a: LaunchArgs,
    flags: DraftFlags,
}

impl Rig {
    fn new(label: &str, model_file: &str, drafter: Option<(&str, &[&str])>) -> Self {
        let dir = tmp(label);
        let model = dir.join(model_file);
        write_gguf(&model, "gemma4", VOCAB, 0);
        if let Some((file, vocab)) = drafter {
            write_gguf(&dir.join("drafters").join(file), "gemma4", vocab, 0);
        }
        let real = gguf::read_meta(&model).unwrap();
        let meta = gemma_12b(real.tokenizer);
        let p = machine(vec![nvidia(24_576, 23_000)], 65_536, 48_000);
        let a = args(&p, &meta, Backend::Cuda, GovState::Normal);
        Rig {
            dir,
            model,
            meta,
            p,
            a,
            flags: ld::parse_help(B11602_HELP),
        }
    }

    fn plan(&self, state: GovState) -> Plan {
        plan(&Launch {
            flags: &self.flags,
            model: &self.model,
            meta: &self.meta,
            profile: &self.p,
            state,
            args: &self.a,
            tune_dir: &self.dir,
            device: "NVIDIA GeForce RTX",
        })
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

const E2B_FILE: &str = "gemma-4-E2B-it-Q4_K_M.gguf";
const G12B_FILE: &str = "gemma-4-12B-it-Q4_K_M.gguf";

#[test]
fn a_matching_catalog_drafter_waits_for_the_speed_test_then_runs() {
    let rig = Rig::new("pair", G12B_FILE, Some((E2B_FILE, VOCAB)));
    let key = "NVIDIA GeForce RTX|gemma-4-12B-it-Q4_K_M.gguf|gemma-4-E2B-it-Q4_K_M.gguf";
    let first = rig.plan(GovState::Normal);
    assert_eq!(first.decision, Decision::NeedsTest);
    assert_eq!(first.draft, None);
    assert_eq!(first.key.as_deref(), Some(key));
    assert_eq!(first.name.as_deref(), Some("Gemma 4 E2B"));
    let keep = Verdict {
        keep: true,
        acceptance: 0.6,
        gain: 1.3,
    };
    ld::save_verdict(&rig.dir, key, keep).unwrap();
    let on = rig.plan(GovState::Normal);
    assert_eq!(on.decision, Decision::On);
    assert_eq!(
        on.draft,
        Some(DraftArgs {
            kind: DraftKind::Simple,
            model: Some(rig.dir.join("drafters").join(E2B_FILE)),
            on_gpu: true,
            flags: rig.flags.clone(),
        })
    );
    assert_eq!(rig.plan(GovState::RampUp).decision, Decision::On);
    assert_eq!(
        rig.plan(GovState::Throttle).decision,
        Decision::Off("the machine is busy, so the draft model is off")
    );
    ld::save_verdict(
        &rig.dir,
        key,
        Verdict {
            keep: false,
            ..keep
        },
    )
    .unwrap();
    assert_eq!(
        rig.plan(GovState::Normal).decision,
        Decision::Off("the speed test found the draft model didn't help")
    );
}

#[test]
fn a_drafter_with_another_vocabulary_is_refused() {
    let rig = Rig::new(
        "vocab",
        G12B_FILE,
        Some((E2B_FILE, &["<pad>", "<eos>", "<bos>", "hello", "world!"])),
    );
    assert_eq!(
        rig.plan(GovState::Normal).decision,
        Decision::Off("the draft model's tokenizer doesn't match the model")
    );
}

#[test]
fn no_drafter_on_disk_is_never_downloaded_for_you() {
    let rig = Rig::new("none", G12B_FILE, None);
    assert_eq!(
        rig.plan(GovState::Normal).decision,
        Decision::Off("no draft model is installed")
    );
    assert!(!rig.dir.join("drafters").exists());
}

#[test]
fn a_community_model_pairs_with_any_drafter_that_tokenizes_the_same() {
    let rig = Rig::new(
        "community",
        "my-gemma-finetune.Q5_K_M.gguf",
        Some(("someone-e2b.gguf", VOCAB)),
    );
    // An unrelated drafter with another vocabulary sorts first and is skipped.
    write_gguf(
        &rig.dir.join("drafters").join("a-llama-drafter.gguf"),
        "llama",
        &["x"],
        0,
    );
    let p = rig.plan(GovState::Normal);
    assert_eq!(p.decision, Decision::NeedsTest);
    assert_eq!(p.name.as_deref(), Some("someone-e2b.gguf"));
    assert_eq!(
        p.key.as_deref(),
        Some("NVIDIA GeForce RTX|my-gemma-finetune.Q5_K_M.gguf|someone-e2b.gguf")
    );
}

#[test]
fn mtp_heads_draft_without_a_second_file() {
    let mut rig = Rig::new("mtp", G12B_FILE, None);
    rig.meta.nextn_layers = 1;
    let p = rig.plan(GovState::Normal);
    assert_eq!(p.decision, Decision::NeedsTest);
    assert_eq!(p.name.as_deref(), Some("the model's own prediction heads"));
    assert_eq!(
        p.key.as_deref(),
        Some("NVIDIA GeForce RTX|gemma-4-12B-it-Q4_K_M.gguf|mtp")
    );
    // An engine without draft-mtp can't use them.
    rig.flags.kinds.retain(|k| k != "draft-mtp");
    assert_eq!(
        rig.plan(GovState::Normal).decision,
        Decision::Off("no draft model is installed")
    );
}

/// A server that answers each request with `body`, `n` times.
fn fake_server(body: &'static str, n: usize) -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in l.incoming().take(n) {
            let mut s = stream.unwrap();
            let mut buf = [0u8; 8192];
            let mut got = Vec::new();
            // Read the headers and the JSON body.
            while !String::from_utf8_lossy(&got).contains("\"stream\"") {
                let k = s.read(&mut buf).unwrap();
                if k == 0 {
                    break;
                }
                got.extend(&buf[..k]);
            }
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            s.write_all(reply.as_bytes()).unwrap();
        }
    });
    url
}

#[test]
fn the_speed_test_reads_speed_and_acceptance_from_the_server() {
    let url = fake_server(
        r#"{"choices":[{"message":{"content":"The sky"}}],"timings":{"predicted_n":128,"predicted_per_second":42.5,"draft_n":120,"draft_n_accepted":78}}"#,
        1,
    );
    assert_eq!(
        measure(&url, "sk-abcdefghijklmnopqrstuv"),
        Ok(Timing {
            tok_s: 42.5,
            drafted: 120,
            accepted: 78
        })
    );
    let bare = fake_server(r#"{"choices":[{"message":{"content":"The sky"}}]}"#, 1);
    assert_eq!(
        measure(&bare, "sk-abcdefghijklmnopqrstuv"),
        Err("the server didn't report its speed".into())
    );
}

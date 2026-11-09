use super::*;

const HELP: &str = "\
----- common params -----

-h,    --help, --usage                  print usage and exit
-t,    --threads N                      number of threads to use during generation (default: -1)
--prio N                                set process/thread priority : low(-1), normal(0), medium(1),
                                        high(2), realtime(3) (default: 0)
--poll <0...100>                        use polling level to wait for work (0 - no polling, default: 50)
-ub,   --ubatch-size N                  physical maximum batch size (default: 512)
-np,   --parallel N                     number of parallel sequences to decode (default: 1)
";

#[test]
fn the_engines_options_are_the_names_that_lead_each_help_line() {
    let o = parse_options(HELP);
    for want in [
        "-h",
        "--help",
        "--usage",
        "-t",
        "--threads",
        "--prio",
        "--poll",
        "-ub",
        "--ubatch-size",
        "-np",
        "--parallel",
    ] {
        assert!(o.contains(want), "missing {want}");
    }
    assert!(!o.contains("-1"));
    assert!(!o.contains("-----"));
    assert_eq!(o.len(), 11);
}

#[test]
fn a_model_sharing_the_gpu_sleeps_runs_low_and_submits_short_batches() {
    let o = parse_options(HELP);
    let f = shared(&o, Backend::Vulkan, true);
    assert_eq!(
        f,
        RunFlags {
            poll: Some(0),
            prio: Some(-1),
            ubatch: Some(128),
            vk_nodes_per_submit: Some(20),
            low_priority: true,
            slots: Slots::default(),
        }
    );
    assert_eq!(
        f.to_args(),
        ["--poll", "0", "--prio", "-1", "--ubatch-size", "128"]
    );
    assert_eq!(
        f.env(),
        vec![("GGML_VK_MAX_NODES_PER_SUBMIT", "20".to_string())]
    );
    // On the CPU the batch size stays and there is no Vulkan queue.
    let cpu = shared(&o, Backend::Cpu, false);
    assert_eq!(cpu.ubatch, None);
    assert_eq!(cpu.vk_nodes_per_submit, None);
    assert_eq!(cpu.to_args(), ["--poll", "0", "--prio", "-1"]);
    // CUDA keeps the batch cap but has no Vulkan variable.
    assert_eq!(shared(&o, Backend::Cuda, true).env(), vec![]);
}

#[test]
fn a_build_without_a_flag_never_gets_it() {
    let old = parse_options("-t, --threads N   threads\n");
    let f = shared(&old, Backend::Vulkan, true);
    assert_eq!(f.to_args(), Vec::<String>::new());
    assert_eq!(
        f.vk_nodes_per_submit,
        Some(20),
        "an env var is safe on any build"
    );
}

/// The pinned engine's own `--help`.
const PINNED_HELP: &str =
    include_str!("../../../grokhub-core/tests/fixtures/llama_server_help_b12017.txt");

#[test]
fn the_pinned_engine_knows_every_slot_and_cache_flag() {
    let o = parse_options(PINNED_HELP);
    for want in [
        "--parallel",
        "--kv-unified",
        "--no-kv-unified",
        "--cache-reuse",
        "--cache-ram",
        "--poll",
        "--prio",
        "--ubatch-size",
    ] {
        assert!(o.contains(want), "missing {want}");
    }
}

#[test]
fn slots_follow_the_state_and_context_with_one_on_the_cpu_or_a_short_context() {
    assert_eq!(slot_count(GovState::RampUp, 65_536, false), 4);
    assert_eq!(slot_count(GovState::RampUp, 16_384, false), 2);
    assert_eq!(slot_count(GovState::Normal, 65_536, false), 2);
    assert_eq!(slot_count(GovState::Throttle, 65_536, false), 1);
    assert_eq!(slot_count(GovState::RampUp, 8_192, false), 1);
    assert_eq!(slot_count(GovState::RampUp, 65_536, true), 1);
}

#[test]
fn more_than_one_slot_shares_one_kv_pool_and_the_prompt_cache_is_reused() {
    let o = parse_options(PINNED_HELP);
    let mut f = RunFlags {
        slots: slots(&o, 2, Some(20_000)),
        ..Default::default()
    };
    assert_eq!(
        f.to_args(),
        [
            "--parallel",
            "2",
            "--kv-unified",
            "--cache-reuse",
            "256",
            "--cache-ram",
            "5000"
        ]
    );
    assert_eq!(f.seqs(), 2);
    f.slots = slots(&o, 1, Some(64_000));
    assert_eq!(
        f.to_args(),
        [
            "--parallel",
            "1",
            "--no-kv-unified",
            "--cache-reuse",
            "256",
            "--cache-ram",
            "8192"
        ]
    );
    // An unknown free RAM keeps the engine's own cap.
    assert_eq!(slots(&o, 1, None).cache_ram_mb, Some(8_192));
    // A build without the flags gets none of them, and one sequence.
    let old = RunFlags {
        slots: slots(&parse_options(HELP), 4, Some(20_000)),
        ..Default::default()
    };
    assert_eq!(old.to_args(), ["--parallel", "4"]);
    let none = RunFlags {
        slots: slots(&parse_options("-t, --threads N\n"), 4, None),
        ..Default::default()
    };
    assert_eq!(none.to_args(), Vec::<String>::new());
    assert_eq!(none.seqs(), 1);
}

#[cfg(target_os = "linux")]
#[test]
fn a_low_priority_child_runs_at_nice_ten() {
    let mut child = Command::new("sleep").arg("30").spawn().unwrap();
    lower_priority(child.id());
    let stat = std::fs::read_to_string(format!("/proc/{}/stat", child.id())).unwrap();
    let after_name = &stat[stat.rfind(')').unwrap() + 2..];
    // Fields after the name: state is field 3, nice is field 19.
    let nice: i32 = after_name.split(' ').nth(16).unwrap().parse().unwrap();
    let _ = child.kill();
    let _ = child.wait();
    assert_eq!(nice, 10);
}

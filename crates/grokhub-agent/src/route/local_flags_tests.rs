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

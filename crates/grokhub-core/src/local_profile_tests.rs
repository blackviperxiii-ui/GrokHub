use super::*;
use crate::local_picker::{preselected, tier};

fn gpu(vendor: GpuVendor, name: &str, vram_mb: u64) -> Gpu {
    Gpu {
        vendor,
        name: name.into(),
        vram_mb,
        free_vram_mb: None,
        integrated: false,
        unified: false,
        driver: String::new(),
    }
}

fn igpu(vendor: GpuVendor, name: &str) -> Gpu {
    Gpu {
        integrated: true,
        ..gpu(vendor, name, 0)
    }
}

fn apple_gpu(ram_mb: u64) -> Gpu {
    Gpu {
        unified: true,
        ..gpu(GpuVendor::Apple, "Apple M-series GPU", ram_mb * 70 / 100)
    }
}

fn machine(os: &'static str, gpus: Vec<Gpu>, ram_mb: u64, simd: CpuSimd) -> LocalProfile {
    LocalProfile {
        os,
        arch: if simd == CpuSimd::Neon {
            "aarch64"
        } else {
            "x86_64"
        },
        gpus,
        cpu: CpuInfo::new(16, 8, 0, simd),
        ram_mb,
        free_ram_mb: Some(ram_mb / 2),
        power: PowerInfo::default(),
        thermal: Thermal::Nominal,
        free_disk_mb: 500_000,
    }
}

const ALL_BACKENDS: [Backend; 6] = BACKEND_ORDER;

fn backend(p: &LocalProfile) -> Backend {
    choose_backend(p, &ALL_BACKENDS, &|_| true)
}

fn default_model(p: &LocalProfile) -> Option<&'static str> {
    preselected(&p.picker_profile(Some(11_602))).map(|m| m.name)
}

fn row(p: &LocalProfile) -> (Backend, &'static str, Option<&'static str>) {
    let pp = p.picker_profile(Some(11_602));
    (backend(p), tier(&pp), default_model(p))
}

#[test]
fn windows_machines_by_vendor() {
    let nvidia = machine(
        "windows",
        vec![gpu(GpuVendor::Nvidia, "NVIDIA GeForce RTX 4070", 12_282)],
        32_768,
        CpuSimd::Avx2,
    );
    assert_eq!(row(&nvidia), (Backend::Cuda, "T3", Some("Gemma 4 12B")));
    let amd = machine(
        "windows",
        vec![gpu(GpuVendor::Amd, "AMD Radeon RX 7800 XT", 16_368)],
        32_768,
        CpuSimd::Avx2,
    );
    assert_eq!(row(&amd), (Backend::Rocm, "T3", Some("Gemma 4 26B-A4B")));
    let arc = machine(
        "windows",
        vec![
            gpu(GpuVendor::Intel, "Intel(R) Arc(TM) A770 Graphics", 16_256),
            igpu(GpuVendor::Intel, "Intel(R) UHD Graphics 770"),
        ],
        32_768,
        CpuSimd::Avx2,
    );
    assert_eq!(row(&arc), (Backend::Vulkan, "T3", Some("Gemma 4 26B-A4B")));
    let igpu_only = machine(
        "windows",
        vec![igpu(GpuVendor::Intel, "Intel(R) Iris(R) Xe Graphics")],
        16_384,
        CpuSimd::Avx2,
    );
    // An iGPU shares RAM: the RAM budget picks, Vulkan may still offload.
    assert_eq!(
        row(&igpu_only),
        (Backend::Vulkan, "T2", Some("Gemma 4 E4B"))
    );
}

#[test]
fn macs_apple_silicon_by_memory_and_intel_with_amd() {
    let cases = [
        (8_192, "T1", "Gemma 4 E2B"),
        (16_384, "T2", "Gemma 4 E4B"),
        (32_768, "T3", "Gemma 4 26B-A4B"),
        (65_536, "T5", "Gemma 4 31B"),
    ];
    for (ram, t, model) in cases {
        let mac = machine("macos", vec![apple_gpu(ram)], ram, CpuSimd::Neon);
        assert_eq!(row(&mac), (Backend::Metal, t, Some(model)), "{ram}");
        assert!(mac.picker_profile(None).unified);
    }
    let intel_mac = machine(
        "macos",
        vec![
            gpu(GpuVendor::Amd, "AMD Radeon Pro 5500M", 8_192),
            igpu(GpuVendor::Intel, "Intel UHD Graphics 630"),
        ],
        16_384,
        CpuSimd::Avx2,
    );
    // ROCm doesn't ship for macOS: an engine without it falls to Vulkan (MoltenVK).
    assert_eq!(
        choose_backend(&intel_mac, &[Backend::Vulkan, Backend::Cpu], &|_| true),
        Backend::Vulkan
    );
    assert_eq!(default_model(&intel_mac), Some("Gemma 4 E4B"));
}

#[test]
fn linux_machines_by_backend() {
    let cuda = machine(
        "linux",
        vec![gpu(GpuVendor::Nvidia, "NVIDIA GeForce RTX 3090", 24_576)],
        65_536,
        CpuSimd::Avx2,
    );
    assert_eq!(row(&cuda), (Backend::Cuda, "T4", Some("Gemma 4 31B")));
    // The example desktop: RX 9070 XT 16 GB + Radeon iGPU, 58 GB RAM.
    let amd = machine(
        "linux",
        vec![
            gpu(GpuVendor::Amd, "AMD Radeon GPU", 16_304),
            igpu(GpuVendor::Amd, "AMD Radeon iGPU"),
        ],
        59_392,
        CpuSimd::Avx2,
    );
    assert_eq!(row(&amd), (Backend::Rocm, "T3", Some("Gemma 4 26B-A4B")));
    assert_eq!(amd.device_plan().gpus.len(), 1, "the iGPU is left out");
    // The same card on an engine without ROCm runs Vulkan.
    assert_eq!(
        choose_backend(&amd, &[Backend::Vulkan, Backend::Cpu], &|_| true),
        Backend::Vulkan
    );
    // ROCm that failed its speed test falls back to Vulkan.
    assert_eq!(
        choose_backend(&amd, &ALL_BACKENDS, &|b| b != Backend::Rocm),
        Backend::Vulkan
    );
    let cpu_only = machine("linux", vec![], 16_384, CpuSimd::Avx2);
    assert_eq!(row(&cpu_only), (Backend::Cpu, "T2", Some("Gemma 4 E4B")));
    let low_end = machine("linux", vec![], 8_192, CpuSimd::Avx2);
    assert_eq!(row(&low_end), (Backend::Cpu, "T1", Some("Gemma 4 E2B")));
    let four_gb = machine("linux", vec![], 4_096, CpuSimd::Avx2);
    assert_eq!(row(&four_gb), (Backend::Cpu, "T0", None));
}

#[test]
fn multi_gpu_splits_by_free_vram_on_one_vendor() {
    let mut a = gpu(GpuVendor::Nvidia, "RTX 4090", 24_576);
    a.free_vram_mb = Some(23_000);
    let mut b = gpu(GpuVendor::Nvidia, "RTX 3060", 12_288);
    b.free_vram_mb = Some(11_000);
    let p = machine(
        "linux",
        vec![
            a,
            b,
            gpu(GpuVendor::Amd, "AMD Radeon GPU", 8_192),
            igpu(GpuVendor::Intel, "Intel iGPU"),
        ],
        65_536,
        CpuSimd::Avx2,
    );
    let plan = p.device_plan();
    assert_eq!(
        plan.gpus
            .iter()
            .map(|g| g.name.as_str())
            .collect::<Vec<_>>(),
        ["RTX 4090", "RTX 3060"]
    );
    assert_eq!(plan.tensor_split, Some(vec![23_000, 11_000]));
    assert_eq!(plan.vram_mb(), 36_864);
    assert_eq!(row(&p), (Backend::Cuda, "T5", Some("Gemma 4 31B")));
    let single = machine(
        "linux",
        vec![gpu(GpuVendor::Nvidia, "RTX 4090", 24_576)],
        65_536,
        CpuSimd::Avx2,
    );
    assert_eq!(single.device_plan().tensor_split, None);
}

#[test]
fn engine_files_name_their_backends() {
    assert_eq!(
        engine_backends(&["llama-server", "libggml-cpu-haswell.so", "libggml-base.so"]),
        [Backend::Cpu]
    );
    assert_eq!(
        engine_backends(&["ggml-cuda.dll", "ggml-vulkan.dll", "ggml-cpu-x64.dll"]),
        [Backend::Cuda, Backend::Vulkan, Backend::Cpu]
    );
    assert_eq!(
        engine_backends(&["libggml-hip.so", "libggml-metal.dylib", "libggml-sycl.so"]),
        [Backend::Rocm, Backend::Metal, Backend::Sycl, Backend::Cpu]
    );
    // The pinned CPU engine on an NVIDIA box still runs on the CPU.
    let p = machine(
        "linux",
        vec![gpu(GpuVendor::Nvidia, "RTX 4070", 12_282)],
        32_768,
        CpuSimd::Avx2,
    );
    assert_eq!(choose_backend(&p, &[Backend::Cpu], &|_| true), Backend::Cpu);
    assert_eq!(Backend::Rocm.label(), "ROCm");
}

#[test]
fn probes_fall_through_to_the_first_reader_that_finds_a_gpu() {
    struct Gpus(Vec<Gpu>);
    impl GpuProbe for Gpus {
        fn gpus(&self) -> Vec<Gpu> {
            self.0.clone()
        }
    }
    struct Fixed;
    impl CpuProbe for Fixed {
        fn cpu(&self) -> CpuInfo {
            CpuInfo::new(24, 16, 8, CpuSimd::Avx2)
        }
    }
    impl MemoryProbe for Fixed {
        fn ram_mb(&self) -> Option<u64> {
            Some(32_768)
        }
        fn free_ram_mb(&self) -> Option<u64> {
            None
        }
        fn free_disk_mb(&self) -> Option<u64> {
            None
        }
    }
    impl PowerProbe for Fixed {
        fn power(&self) -> Option<PowerInfo> {
            None
        }
    }
    impl ThermalProbe for Fixed {
        fn thermal(&self) -> Thermal {
            Thermal::Unknown
        }
    }
    let nvml = Gpus(vec![]);
    let drm = Gpus(vec![gpu(GpuVendor::Amd, "AMD Radeon GPU", 16_384)]);
    let vulkan = Gpus(vec![gpu(GpuVendor::Other, "llvmpipe", 0)]);
    let p = profile(&Probes {
        gpu: &[&nvml, &drm, &vulkan],
        cpu: &Fixed,
        memory: &Fixed,
        power: &Fixed,
        thermal: &Fixed,
    });
    assert_eq!(p.gpus, vec![gpu(GpuVendor::Amd, "AMD Radeon GPU", 16_384)]);
    assert_eq!((p.cpu.p_cores, p.cpu.e_cores), (8, 8));
    assert_eq!(p.ram_mb, 32_768);
    // Missing readings take the conservative defaults.
    assert_eq!(p.free_disk_mb, 0);
    assert_eq!(p.power, PowerInfo::default());
    assert!(p.power.on_ac && !p.power.battery);
}

#[test]
fn nvidia_smi_lists_every_gpu_with_free_vram_and_driver() {
    let text = "NVIDIA GeForce RTX 4090, 24564, 23010, 580.95.05\nNVIDIA GeForce RTX 3060, 12288, 11800, 580.95.05\n\nbad line\n";
    let gpus = parse_nvidia_smi_all(text);
    assert_eq!(gpus.len(), 2);
    assert_eq!(
        gpus[0],
        Gpu {
            vendor: GpuVendor::Nvidia,
            name: "NVIDIA GeForce RTX 4090".into(),
            vram_mb: 24_564,
            free_vram_mb: Some(23_010),
            integrated: false,
            unified: false,
            driver: "580.95.05".into(),
        }
    );
    assert_eq!(parse_nvidia_smi_all("NVIDIA-SMI has failed"), vec![]);
}

#[test]
fn drm_sysfs_reads_amd_cards_apus_and_intel_arc() {
    const GIB: u64 = 1_024 * 1_024 * 1_024;
    let cards = [
        DrmCard {
            vendor: "0x1002".into(),
            driver: "amdgpu".into(),
            vram_total: Some(16 * GIB),
            vram_used: Some(GIB),
            lmem_total: None,
        },
        DrmCard {
            vendor: "0x1002".into(),
            driver: "amdgpu".into(),
            vram_total: Some(GIB / 2),
            vram_used: None,
            lmem_total: None,
        },
        DrmCard {
            vendor: "0x8086".into(),
            driver: "xe".into(),
            vram_total: None,
            vram_used: None,
            lmem_total: Some(12 * GIB),
        },
        DrmCard {
            vendor: "0x8086".into(),
            driver: "i915".into(),
            ..DrmCard::default()
        },
        DrmCard {
            vendor: "0x10de".into(),
            driver: "nvidia".into(),
            ..DrmCard::default()
        },
    ];
    let gpus = gpus_from_drm(&cards);
    let got: Vec<(&str, u64, Option<u64>, bool)> = gpus
        .iter()
        .map(|g| (g.name.as_str(), g.vram_mb, g.free_vram_mb, g.integrated))
        .collect();
    assert_eq!(
        got,
        [
            ("AMD Radeon GPU", 16_384, Some(15_360), false),
            ("AMD Radeon iGPU", 512, None, true),
            ("Intel Arc GPU", 12_288, None, false),
            ("Intel iGPU", 0, None, true),
        ]
    );
}

#[test]
fn windows_adapters_read_64_bit_memory_and_spot_igpus() {
    let csv = "\"Name\",\"Bytes\"\n\"NVIDIA GeForce RTX 4080\",\"17171480576\"\n\"Intel(R) UHD Graphics 770\",\"134217728\"\n\"AMD Radeon(TM) Graphics\",\"536870912\"\n\"Intel(R) Arc(TM) B580 Graphics\",\"12868124672\"\n\"Microsoft Basic Display Adapter\",\"0\"\n";
    let got: Vec<(GpuVendor, u64, bool)> = parse_windows_gpus(csv)
        .iter()
        .map(|g| (g.vendor, g.vram_mb, g.integrated))
        .collect();
    assert_eq!(
        got,
        [
            (GpuVendor::Nvidia, 16_376, false),
            (GpuVendor::Intel, 0, true),
            (GpuVendor::Amd, 0, true),
            (GpuVendor::Intel, 12_272, false),
        ]
    );
}

#[test]
fn cpu_simd_core_lists_and_hybrid_splits() {
    assert_eq!(
        parse_cpuinfo_simd("processor\t: 0\nflags\t\t: fpu sse4_2 avx avx2 fma\n"),
        CpuSimd::Avx2
    );
    assert_eq!(
        parse_cpuinfo_simd("flags\t\t: fpu sse4_2 avx\n"),
        CpuSimd::AvxOnly
    );
    assert_eq!(
        parse_cpuinfo_simd("flags\t\t: fpu sse4_2\n"),
        CpuSimd::NoAvx
    );
    assert_eq!(
        parse_cpuinfo_simd("Features\t: fp asimd evtstrm aes\n"),
        CpuSimd::Neon
    );
    assert_eq!(parse_cpu_list("0-15\n"), 16);
    assert_eq!(parse_cpu_list("0-7,16-23"), 16);
    assert_eq!(parse_cpu_list("3"), 1);
    assert_eq!(parse_cpu_list(""), 0);
    // i7-12700H: 6 P-cores (12 threads) + 8 E-cores = 20 threads, 14 cores.
    let hybrid = CpuInfo::new(20, 14, 8, CpuSimd::Avx2);
    assert_eq!((hybrid.p_cores, hybrid.e_cores), (6, 8));
    let plain = CpuInfo::new(32, 16, 0, CpuSimd::Avx2);
    assert_eq!((plain.p_cores, plain.e_cores), (16, 0));
    // A bad reading never leaves zero P-cores.
    assert_eq!(CpuInfo::new(0, 0, 4, CpuSimd::Avx2).p_cores, 1);
}

#[test]
fn laptop_power_and_thermal_readings() {
    let bat = |status: &str, cap: u8| PowerSupply {
        kind: "Battery".into(),
        online: None,
        capacity: Some(cap),
        status: Some(status.into()),
    };
    let mains = |on: bool| PowerSupply {
        kind: "Mains".into(),
        online: Some(on),
        ..PowerSupply::default()
    };
    assert_eq!(
        power_from_supplies(&[bat("Discharging", 41), mains(false)], Some("balanced")),
        PowerInfo {
            battery: true,
            on_ac: false,
            percent: Some(41),
            power_saver: false,
        }
    );
    assert_eq!(
        power_from_supplies(&[bat("Charging", 80), mains(true)], Some("low-power\n")),
        PowerInfo {
            battery: true,
            on_ac: true,
            percent: Some(80),
            power_saver: true,
        }
    );
    assert_eq!(power_from_supplies(&[], None), PowerInfo::default());
    assert_eq!(
        parse_windows_battery("\"BatteryStatus\",\"EstimatedChargeRemaining\"\n\"1\",\"17\"\n"),
        PowerInfo {
            battery: true,
            on_ac: false,
            percent: Some(17),
            power_saver: false,
        }
    );
    assert!(
        parse_windows_battery("\"BatteryStatus\",\"EstimatedChargeRemaining\"\n\"2\",\"99\"\n")
            .on_ac
    );
    assert_eq!(parse_windows_battery(""), PowerInfo::default());

    let z = |t: &str, m: i64| (t.to_string(), m);
    assert_eq!(
        thermal_from_zones(&[z("acpitz", 41_000), z("x86_pkg_temp", 97_000)]),
        Thermal::Critical
    );
    assert_eq!(
        thermal_from_zones(&[z("x86_pkg_temp", 88_000)]),
        Thermal::Serious
    );
    assert_eq!(
        thermal_from_zones(&[z("x86_pkg_temp", 76_000)]),
        Thermal::Fair
    );
    assert_eq!(
        thermal_from_zones(&[z("x86_pkg_temp", 52_000)]),
        Thermal::Nominal
    );
    // Sensors that read 0 or nonsense are ignored.
    assert_eq!(
        thermal_from_zones(&[z("bogus", 0), z("bogus", 200_000)]),
        Thermal::Unknown
    );
    assert_eq!(thermal_from_zones(&[]), Thermal::Unknown);
}

#[test]
fn vendor_ids_and_names() {
    assert_eq!(GpuVendor::from_pci_id("0x10DE\n"), GpuVendor::Nvidia);
    assert_eq!(GpuVendor::from_pci_id("0x1002"), GpuVendor::Amd);
    assert_eq!(GpuVendor::from_pci_id("0x8086"), GpuVendor::Intel);
    assert_eq!(GpuVendor::from_pci_id("0x1af4"), GpuVendor::Other);
    assert_eq!(
        GpuVendor::from_name("AMD Radeon RX 9070 XT"),
        GpuVendor::Amd
    );
    assert_eq!(
        GpuVendor::from_name("NVIDIA GeForce GTX 1060"),
        GpuVendor::Nvidia
    );
}

#[test]
fn vulkan_summary_names_devices_and_skips_software_renderers() {
    let text = "Devices:\n========\nGPU0:\n\tapiVersion = 1.4.311\n\tvendorID = 0x1002\n\tdeviceID = 0x7550\n\tdeviceType = PHYSICAL_DEVICE_TYPE_DISCRETE_GPU\n\tdeviceName = AMD Radeon RX 9070 XT (RADV GFX1201)\n\tdriverInfo = Mesa 25.2.4\nGPU1:\n\tvendorID = 0x1002\n\tdeviceType = PHYSICAL_DEVICE_TYPE_INTEGRATED_GPU\n\tdeviceName = AMD Ryzen 9 9950X3D Radeon Graphics (RADV RAPHAEL_MENDOCINO)\nGPU2:\n\tvendorID = 0x10005\n\tdeviceType = PHYSICAL_DEVICE_TYPE_CPU\n\tdeviceName = llvmpipe (LLVM 20.1.8, 256 bits)\n";
    let gpus = parse_vulkan_summary(text);
    let got: Vec<(&str, GpuVendor, bool, &str)> = gpus
        .iter()
        .map(|g| (g.name.as_str(), g.vendor, g.integrated, g.driver.as_str()))
        .collect();
    assert_eq!(
        got,
        [
            (
                "AMD Radeon RX 9070 XT (RADV GFX1201)",
                GpuVendor::Amd,
                false,
                "Mesa 25.2.4"
            ),
            (
                "AMD Ryzen 9 9950X3D Radeon Graphics (RADV RAPHAEL_MENDOCINO)",
                GpuVendor::Amd,
                true,
                ""
            ),
        ]
    );
    assert_eq!(
        parse_meminfo_mb(
            "MemTotal: 16314252 kB\nMemAvailable:  9437184 kB\n",
            "MemAvailable:"
        ),
        Some(9_216)
    );
    assert_eq!(parse_meminfo_mb("MemTotal: 1 kB\n", "MemAvailable:"), None);
}

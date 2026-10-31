//! The PC loadout tile's DETAILS sheet: everything the hardware probe read,
//! from the same cached profile the tile shows (no extra reads).

use eframe::egui::{self, RichText};
use grokhub_core::local_picker::CpuSimd;
use grokhub_core::local_profile::{Gpu, LocalProfile, Thermal};

const OPEN_ID: &str = "home-pc-sheet-open";

fn gb(mb: u64) -> String {
    if mb < 1_024 {
        format!("{mb} MB")
    } else {
        format!("{} GB", (mb + 512) / 1_024)
    }
}

fn gpu_line(g: &Gpu) -> String {
    let kind = if g.unified {
        "Unified memory"
    } else if g.integrated {
        "Integrated"
    } else {
        "Graphics card"
    };
    let mem = match (g.unified, g.integrated, g.vram_mb, g.free_vram_mb) {
        (true, _, mb, _) if mb > 0 => Some(format!("up to {} of RAM", gb(mb))),
        (true, _, _, _) => Some("shares RAM".into()),
        (_, true, 0, _) => Some("shares RAM".into()),
        (_, true, mb, _) => Some(format!("{} carved from RAM", gb(mb))),
        (_, false, 0, _) => None,
        (_, false, mb, Some(free)) => Some(format!("{} VRAM, {} free", gb(mb), gb(free))),
        (_, false, mb, None) => Some(format!("{} VRAM", gb(mb))),
    };
    let mut parts = vec![kind.to_string()];
    parts.extend(mem);
    let driver = g.driver.trim();
    if !driver.is_empty() {
        parts.push(format!("driver {driver}"));
    }
    parts.join(" · ")
}

/// Label and value rows for the sheet, in reading order. Unread facts are left out.
pub(super) fn pc_sheet_lines(p: &LocalProfile) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    for g in &p.gpus {
        let name = g.name.trim();
        if !name.is_empty() {
            rows.push((name.to_string(), gpu_line(g)));
        }
    }
    let c = &p.cpu;
    if c.logical > 0 {
        let mut v = format!("{} cores, {} threads", c.physical.max(1), c.logical);
        if c.e_cores > 0 {
            v.push_str(&format!(
                " ({} performance + {} efficient)",
                c.p_cores, c.e_cores
            ));
        }
        v.push_str(match c.simd {
            CpuSimd::Avx2 => " · AVX2",
            CpuSimd::AvxOnly => " · AVX",
            CpuSimd::NoAvx => " · no AVX",
            CpuSimd::Neon => " · NEON",
        });
        rows.push(("CPU".into(), v));
    }
    if p.ram_mb > 0 {
        let v = match p.free_ram_mb {
            Some(free) => format!("{} ({} free)", gb(p.ram_mb), gb(free)),
            None => gb(p.ram_mb),
        };
        rows.push(("RAM".into(), v));
    }
    if p.free_disk_mb > 0 {
        rows.push(("Disk free".into(), gb(p.free_disk_mb)));
    }
    let pw = &p.power;
    let power = if pw.battery {
        let mut v = match pw.percent {
            Some(pct) => format!("Battery {pct}%"),
            None => "Battery".into(),
        };
        v.push_str(if pw.on_ac {
            " · plugged in"
        } else {
            " · on battery"
        });
        if pw.power_saver {
            v.push_str(" · power saver");
        }
        v
    } else if pw.power_saver {
        "Plugged in · power saver".into()
    } else {
        "Plugged in".into()
    };
    rows.push(("Power".into(), power));
    let heat = match p.thermal {
        Thermal::Unknown => None,
        Thermal::Nominal => Some("Cool"),
        Thermal::Fair => Some("Warm"),
        Thermal::Serious => Some("Hot"),
        Thermal::Critical => Some("Critical"),
    };
    if let Some(h) = heat {
        rows.push(("Thermal".into(), h.into()));
    }
    rows
}

pub(super) fn open_pc_sheet(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(OPEN_ID), true));
}

/// Paint the sheet while it's open. Esc, the backdrop or × closes it.
pub(super) fn show_pc_sheet(ctx: &egui::Context, profile: Option<&LocalProfile>) {
    let id = egui::Id::new(OPEN_ID);
    if !ctx.data(|d| d.get_temp::<bool>(id).unwrap_or(false)) {
        return;
    }
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::new("home-pc-sheet"))
        .backdrop_color(egui::Color32::from_black_alpha(153))
        .frame(
            egui::Frame::NONE
                .fill(crate::theme::elevated())
                .stroke(egui::Stroke::new(1.0_f32, crate::theme::border_strong()))
                .corner_radius(16.0)
                .inner_margin(egui::Margin::same(22)),
        )
        .show(ctx, |ui| {
            ui.set_width(480.0_f32.min(ctx.content_rect().width() - 80.0));
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("This PC")
                        .font(crate::theme::title_font(22.0))
                        .color(crate::theme::fg()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let x = ui
                        .add(
                            egui::Button::new(RichText::new("×").size(18.0))
                                .frame(false)
                                .min_size(egui::vec2(28.0, 28.0)),
                        )
                        .on_hover_text("Close (Esc)");
                    close |= x.clicked();
                });
            });
            ui.add_space(8.0);
            match profile {
                None => {
                    ui.label(
                        RichText::new("Reading this PC…")
                            .size(crate::theme::TYPE_BODY)
                            .color(crate::theme::muted()),
                    );
                }
                Some(p) => {
                    for (label, value) in pc_sheet_lines(p) {
                        ui.label(
                            RichText::new(label)
                                .size(crate::theme::TYPE_CAPTION)
                                .color(crate::theme::muted()),
                        );
                        ui.label(
                            RichText::new(value)
                                .size(crate::theme::TYPE_BODY)
                                .color(crate::theme::fg()),
                        );
                        ui.add_space(6.0);
                    }
                }
            }
        });
    if close || modal.should_close() {
        ctx.data_mut(|d| d.remove::<bool>(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::local_profile::{CpuInfo, GpuVendor, PowerInfo};

    fn gpu(name: &str, vram_mb: u64, free: Option<u64>, integrated: bool, driver: &str) -> Gpu {
        Gpu {
            vendor: GpuVendor::Amd,
            name: name.into(),
            vram_mb,
            free_vram_mb: free,
            integrated,
            unified: false,
            driver: driver.into(),
        }
    }

    #[test]
    fn the_sheet_lists_each_gpu_and_the_rest_of_the_pc() {
        let p = LocalProfile {
            os: "linux",
            arch: "x86_64",
            gpus: vec![
                gpu(
                    "AMD Radeon Graphics (Raphael)",
                    4_096,
                    None,
                    true,
                    "amdgpu (kernel)",
                ),
                gpu(
                    "AMD Radeon RX 9070/9070 XT/9070 GRE",
                    16_384,
                    Some(15_360),
                    false,
                    "amdgpu (kernel)",
                ),
            ],
            cpu: CpuInfo {
                logical: 32,
                physical: 16,
                p_cores: 16,
                e_cores: 0,
                simd: CpuSimd::Avx2,
            },
            ram_mb: 65_536,
            free_ram_mb: Some(40_960),
            power: PowerInfo::default(),
            thermal: Thermal::Unknown,
            free_disk_mb: 512_000,
        };
        let rows = pc_sheet_lines(&p);
        let got: Vec<(&str, &str)> = rows.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        assert_eq!(
            got,
            [
                (
                    "AMD Radeon Graphics (Raphael)",
                    "Integrated · 4 GB carved from RAM · driver amdgpu (kernel)"
                ),
                (
                    "AMD Radeon RX 9070/9070 XT/9070 GRE",
                    "Graphics card · 16 GB VRAM, 15 GB free · driver amdgpu (kernel)"
                ),
                ("CPU", "16 cores, 32 threads · AVX2"),
                ("RAM", "64 GB (40 GB free)"),
                ("Disk free", "500 GB"),
                ("Power", "Plugged in"),
            ]
        );
    }
}

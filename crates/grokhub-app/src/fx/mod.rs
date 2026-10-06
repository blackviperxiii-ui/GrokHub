//! Optional wgpu composer glow. Compiled only with the `fx` feature.

mod guard;

use eframe::egui;
use eframe::egui_wgpu::CallbackTrait;
use std::sync::Mutex;

const COMPOSER_GLOW_WGSL: &str = include_str!("composer_glow.wgsl");
const UNIFORM_BYTES: u64 = 64;

struct Session {
    wgpu: bool,
    notice: Option<&'static str>,
    frame: guard::FirstFrame,
    toasted: bool,
}

static SESSION: Mutex<Session> = Mutex::new(Session {
    wgpu: false,
    notice: None,
    frame: guard::FirstFrame::idle(),
    toasted: false,
});

fn session() -> std::sync::MutexGuard<'static, Session> {
    SESSION.lock().unwrap_or_else(|err| err.into_inner())
}

/// Read the switch, the crash marker, and `GROKHUB_RENDERER`. Write the marker before wgpu starts.
pub(crate) fn prepare_launch() -> eframe::Renderer {
    let mut cfg = crate::config::load();
    let dir = crate::config::config_dir();
    let marker = guard::marker_exists(&dir);
    let raw = std::env::var("GROKHUB_RENDERER").ok();
    let decision = guard::decide(cfg.composer_glow, marker, guard::parse_env(raw.as_deref()));
    let before = guard::Persisted {
        composer_glow: cfg.composer_glow,
        marker,
        notice: false,
    };
    let after = guard::apply_decision(before, &decision);
    sync_disk(&dir, &mut cfg, before, after);
    let mut state = session();
    state.wgpu = false;
    state.notice = after.notice.then_some(guard::FALLBACK_NOTICE);
    state.frame = if decision.launch == guard::Launch::Wgpu {
        guard::FirstFrame::arm()
    } else {
        guard::FirstFrame::idle()
    };
    state.toasted = false;
    drop(state);
    if decision.launch == guard::Launch::Wgpu {
        eframe::Renderer::Wgpu
    } else {
        eframe::Renderer::Glow
    }
}

/// Pipeline and uniform buffer, stored once on the egui wgpu renderer.
pub(crate) fn install(state: &eframe::egui_wgpu::RenderState) {
    let gpu = ComposerGlowGpu::new(&state.device, state.target_format);
    state.renderer.write().callback_resources.insert(gpu);
    session().wgpu = true;
}

/// Write the marker and the switch only where the guard changed them.
fn sync_disk(
    dir: &std::path::Path,
    cfg: &mut crate::config::AppConfig,
    before: guard::Persisted,
    after: guard::Persisted,
) {
    if after.marker {
        let _ = guard::write_marker(dir);
    } else if before.marker {
        let _ = guard::clear_marker(dir);
    }
    if after.composer_glow != before.composer_glow {
        cfg.composer_glow = after.composer_glow;
        let _ = crate::config::save(cfg);
    }
}

/// Adapter, device, or surface creation failed. Stay on glow and remember that.
pub(crate) fn note_wgpu_failed() {
    let dir = crate::config::config_dir();
    let mut cfg = crate::config::load();
    let before = guard::Persisted {
        composer_glow: cfg.composer_glow,
        marker: guard::marker_exists(&dir),
        notice: false,
    };
    let after = guard::fallback_after_error(before);
    sync_disk(&dir, &mut cfg, before, after);
    let mut state = session();
    state.wgpu = false;
    state.notice = after.notice.then_some(guard::FALLBACK_NOTICE);
    state.frame = guard::FirstFrame::idle();
    state.toasted = false;
}

pub(crate) fn renderer_is_wgpu() -> bool {
    session().wgpu
}

pub(crate) fn settings_caption() -> &'static str {
    session().notice.unwrap_or(guard::SETTINGS_CAPTION)
}

pub(crate) fn dismiss_notice() {
    session().notice = None;
}

/// Clear the crash marker once the first wgpu frame was presented, and surface the fallback line once.
pub(crate) fn on_frame() -> Option<&'static str> {
    let dir = crate::config::config_dir();
    let line = {
        let mut state = session();
        guard::clear_on_first_frame(&mut state.frame, &dir);
        if state.notice.is_some() && !state.toasted {
            state.toasted = true;
            state.notice
        } else {
            None
        }
    };
    if let Some(line) = line {
        #[cfg(not(test))]
        crate::notify::ping("GrokHub", line);
        return Some(line);
    }
    None
}

struct ComposerGlowCallback {
    pill_min: [f32; 2],
    pill_max: [f32; 2],
    radius_pts: f32,
    rgb: [f32; 3],
    time: f32,
    intensity: f32,
    animate: bool,
}

impl CallbackTrait for ComposerGlowCallback {
    fn prepare(
        &self,
        _device: &eframe::wgpu::Device,
        queue: &eframe::wgpu::Queue,
        screen: &eframe::egui_wgpu::ScreenDescriptor,
        _encoder: &mut eframe::wgpu::CommandEncoder,
        resources: &mut eframe::egui_wgpu::CallbackResources,
    ) -> Vec<eframe::wgpu::CommandBuffer> {
        if let Some(gpu) = resources.get::<ComposerGlowGpu>() {
            let bytes = pack_glow_uniforms(
                &GlowParams {
                    pill_min: self.pill_min,
                    pill_max: self.pill_max,
                    radius_pts: self.radius_pts,
                    rgb: self.rgb,
                    time: self.time,
                    intensity: self.intensity,
                    animate: self.animate,
                    expand_pts: guard::GLOW_EXPAND,
                },
                screen,
            );
            queue.write_buffer(&gpu.uniform, 0, &bytes);
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::epaint::PaintCallbackInfo,
        pass: &mut eframe::wgpu::RenderPass<'static>,
        resources: &eframe::egui_wgpu::CallbackResources,
    ) {
        let Some(gpu) = resources.get::<ComposerGlowGpu>() else {
            return;
        };
        pass.set_pipeline(&gpu.pipeline);
        pass.set_bind_group(0, &gpu.bind_group, &[]);
        pass.draw(0..6, 0..1);
    }
}

struct ComposerGlowGpu {
    pipeline: eframe::wgpu::RenderPipeline,
    bind_group: eframe::wgpu::BindGroup,
    uniform: eframe::wgpu::Buffer,
}

impl ComposerGlowGpu {
    fn new(device: &eframe::wgpu::Device, format: eframe::wgpu::TextureFormat) -> Self {
        let uniform = device.create_buffer(&eframe::wgpu::BufferDescriptor {
            label: Some("composer_glow_uniforms"),
            size: UNIFORM_BYTES,
            usage: eframe::wgpu::BufferUsages::UNIFORM | eframe::wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group_layout = uniform_layout(device);
        let bind_group = device.create_bind_group(&eframe::wgpu::BindGroupDescriptor {
            label: Some("composer_glow_bind_group"),
            layout: &bind_group_layout,
            entries: &[eframe::wgpu::BindGroupEntry {
                binding: 0,
                resource: eframe::wgpu::BindingResource::Buffer(eframe::wgpu::BufferBinding {
                    buffer: &uniform,
                    offset: 0,
                    size: None,
                }),
            }],
        });
        let pipeline = glow_pipeline(device, format, &bind_group_layout);
        Self {
            pipeline,
            bind_group,
            uniform,
        }
    }
}

fn uniform_layout(device: &eframe::wgpu::Device) -> eframe::wgpu::BindGroupLayout {
    device.create_bind_group_layout(&eframe::wgpu::BindGroupLayoutDescriptor {
        label: Some("composer_glow_layout"),
        entries: &[eframe::wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: eframe::wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: eframe::wgpu::BindingType::Buffer {
                ty: eframe::wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: std::num::NonZeroU64::new(UNIFORM_BYTES),
            },
            count: None,
        }],
    })
}

fn glow_pipeline(
    device: &eframe::wgpu::Device,
    format: eframe::wgpu::TextureFormat,
    layout: &eframe::wgpu::BindGroupLayout,
) -> eframe::wgpu::RenderPipeline {
    let pipeline_layout = device.create_pipeline_layout(&eframe::wgpu::PipelineLayoutDescriptor {
        label: Some("composer_glow_pipeline_layout"),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    let shader = device.create_shader_module(eframe::wgpu::ShaderModuleDescriptor {
        label: Some("composer_glow"),
        source: eframe::wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(COMPOSER_GLOW_WGSL)),
    });
    device.create_render_pipeline(&eframe::wgpu::RenderPipelineDescriptor {
        label: Some("composer_glow_pipeline"),
        layout: Some(&pipeline_layout),
        vertex: eframe::wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: eframe::wgpu::PipelineCompilationOptions::default(),
            buffers: &[],
        },
        primitive: eframe::wgpu::PrimitiveState {
            topology: eframe::wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: eframe::wgpu::FrontFace::Ccw,
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: eframe::wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: None,
        multisample: eframe::wgpu::MultisampleState::default(),
        fragment: Some(eframe::wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: eframe::wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(eframe::wgpu::ColorTargetState {
                format,
                blend: Some(premultiplied_blend()),
                write_mask: eframe::wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn premultiplied_blend() -> eframe::wgpu::BlendState {
    eframe::wgpu::BlendState {
        color: eframe::wgpu::BlendComponent {
            src_factor: eframe::wgpu::BlendFactor::One,
            dst_factor: eframe::wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: eframe::wgpu::BlendOperation::Add,
        },
        alpha: eframe::wgpu::BlendComponent {
            src_factor: eframe::wgpu::BlendFactor::OneMinusDstAlpha,
            dst_factor: eframe::wgpu::BlendFactor::One,
            operation: eframe::wgpu::BlendOperation::Add,
        },
    }
}

pub(crate) struct GlowParams {
    pub pill_min: [f32; 2],
    pub pill_max: [f32; 2],
    pub radius_pts: f32,
    pub rgb: [f32; 3],
    pub time: f32,
    pub intensity: f32,
    pub animate: bool,
    pub expand_pts: f32,
}

pub(crate) fn pack_glow_uniforms(
    params: &GlowParams,
    screen: &eframe::egui_wgpu::ScreenDescriptor,
) -> [u8; UNIFORM_BYTES as usize] {
    let ppp = screen.pixels_per_point;
    let animate = if params.animate { 1.0 } else { 0.0 };
    let values = [
        params.pill_min[0] * ppp,
        params.pill_min[1] * ppp,
        params.pill_max[0] * ppp,
        params.pill_max[1] * ppp,
        params.rgb[0],
        params.rgb[1],
        params.rgb[2],
        params.intensity,
        params.radius_pts * ppp,
        params.time,
        screen.size_in_pixels[0] as f32,
        screen.size_in_pixels[1] as f32,
        animate,
        params.expand_pts * ppp,
        0.0,
        0.0,
    ];
    let mut out = [0_u8; UNIFORM_BYTES as usize];
    for (index, value) in values.iter().enumerate() {
        let start = index * 4;
        out[start..start + 4].copy_from_slice(&value.to_le_bytes());
    }
    out
}

/// Behind the pill. White breath rim (#e7e9ea). Uses [`crate::icons::composer_breath`].
/// Idle α≈0.25; streaming α 0.25↔0.55; settles to idle in GLOW_SETTLE_SECS when stream ends.
/// Skips animation when [`crate::theme::motion_ok`] is false.
/// `streaming` false = settle toward idle α (caller keeps painting briefly after run ends).
pub(crate) fn paint_composer_glow_at(ui: &egui::Ui, pill: egui::Rect, streaming: bool) {
    let motion = crate::theme::motion_ok(ui);
    if motion {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(33));
    }
    let breath = if motion && streaming {
        crate::icons::composer_breath(ui.input(|input| input.time) as f32)
    } else {
        0.0
    };
    // Critiquito: idle 0.25; streaming pulses 0.25↔0.55 via icons breath.
    let stream_alpha = 0.25 + 0.30 * breath;
    let target_idle = 0.25_f32;
    let live_t = crate::theme::animate_selection_secs(
        ui,
        egui::Id::new("composer-glow-stream"),
        streaming,
        grokhub_core::GLOW_SETTLE_SECS,
    );
    let intensity = if motion {
        target_idle + (stream_alpha - target_idle) * live_t
    } else if streaming {
        0.40
    } else {
        target_idle
    };
    let accent = crate::theme::composer_glow_rgb();
    let callback = eframe::egui_wgpu::Callback::new_paint_callback(
        pill.expand(guard::GLOW_EXPAND),
        ComposerGlowCallback {
            pill_min: [pill.min.x, pill.min.y],
            pill_max: [pill.max.x, pill.max.y],
            radius_pts: crate::theme::QUERY_RADIUS,
            rgb: [
                f32::from(accent.r()) / 255.0,
                f32::from(accent.g()) / 255.0,
                f32::from(accent.b()) / 255.0,
            ],
            time: 0.0,
            intensity,
            animate: false, // intensity already carries icons.rs breath; WGSL must not double-pulse
        },
    );
    ui.ctx().layer_painter(ui.layer_id()).add(callback);
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shader_source_has_vertex_and_fragment() {
        assert!(COMPOSER_GLOW_WGSL.contains("@vertex"));
        assert!(COMPOSER_GLOW_WGSL.contains("@fragment"));
        assert!(COMPOSER_GLOW_WGSL.contains("fn vs_main"));
        assert!(COMPOSER_GLOW_WGSL.contains("fn fs_main"));
    }

    
    #[test]
    fn composer_glow_breath_alphas_and_icons_math() {
        assert_eq!(crate::theme::FG, egui::Color32::from_rgb(0xe7, 0xe9, 0xea));
        assert_eq!(grokhub_core::GLOW_SETTLE_SECS, 0.20);
        assert_eq!(grokhub_core::ALWAYS_SETTLE_SECS, 0.14);
        let src = include_str!("mod.rs");
        assert!(
            src.contains("composer_breath")
                && src.contains("0.25 + 0.30 * breath")
                && src.contains("GLOW_SETTLE_SECS")
                && src.contains("animate: false"),
            "glow must use icons breath + Critiquito alphas, no WGSL double pulse: {src}"
        );
        let breath0 = crate::icons::composer_breath(0.0);
        assert!((0.0..=1.0).contains(&breath0));
    }

    #[test]
    fn composer_glow_uses_white_not_live_green() {
        let accent = crate::theme::composer_glow_rgb();
        crate::theme::set_paint_dark(true);
        assert_eq!(crate::theme::composer_glow_rgb(), crate::theme::FG);
        assert_ne!(crate::theme::composer_glow_rgb(), crate::theme::LIVE);
        crate::theme::set_paint_dark(false);
        assert_eq!(crate::theme::composer_glow_rgb(), crate::theme::LIGHT_FG);
        crate::theme::set_paint_dark(true);
        let _ = accent;
        // Only scan production source — this test body must not contain the forbid needle.
        let src = include_str!("mod.rs");
        let prod = src.split("#[cfg(test)]").next().expect("prod");
        let forbid = format!("{}{}", "let accent = crate::theme::", "live()");
        assert!(
            prod.contains("composer_glow_rgb()") && !prod.contains(&forbid),
            "glow must take composer_glow_rgb, not live green: {prod}"
        );
    }

    #[test]
    fn pack_glow_uniforms_lays_out_64_bytes() {
        let screen = eframe::egui_wgpu::ScreenDescriptor {
            size_in_pixels: [400, 200],
            pixels_per_point: 2.0,
        };
        let bytes = pack_glow_uniforms(
            &GlowParams {
                pill_min: [10.0, 20.0],
                pill_max: [110.0, 80.0],
                radius_pts: 160.0,
                rgb: [0.25, 0.5, 1.0],
                time: 0.4,
                intensity: 0.8,
                animate: true,
                expand_pts: 18.0,
            },
            &screen,
        );
        assert_eq!(bytes.len(), 64);
        let slot =
            |index: usize| f32::from_le_bytes(bytes[index * 4..index * 4 + 4].try_into().unwrap());
        assert_eq!(slot(0), 20.0);
        assert_eq!(slot(1), 40.0);
        assert_eq!(slot(2), 220.0);
        assert_eq!(slot(3), 160.0);
        assert_eq!(slot(4), 0.25);
        assert_eq!(slot(5), 0.5);
        assert_eq!(slot(6), 1.0);
        assert_eq!(slot(7), 0.8);
        assert_eq!(slot(8), 320.0);
        assert_eq!(slot(9), 0.4);
        assert_eq!(slot(10), 400.0);
        assert_eq!(slot(11), 200.0);
        assert_eq!(slot(12), 1.0);
        assert_eq!(slot(13), 36.0);
        assert_eq!(slot(14), 0.0);
        assert_eq!(slot(15), 0.0);
    }
}

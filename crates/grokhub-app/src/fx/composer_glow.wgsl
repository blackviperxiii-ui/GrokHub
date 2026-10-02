// Soft glow outside the composer pill. The fragment writes premultiplied alpha.
// rect: pill min.xy max.zw in framebuffer pixels (origin top-left, y down).
// color: theme accent rgb plus intensity. params: radius px, time seconds, screen size.
// flags: animate (> 0.5 breathes over 1.6 s), falloff width in pixels, unused, unused.

struct Glow {
    rect: vec4<f32>,
    color: vec4<f32>,
    params: vec4<f32>,
    flags: vec4<f32>,
}

@group(0) @binding(0)
var<uniform> glow: Glow;

struct VsOut {
    @builtin(position) position: vec4<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VsOut {
    // Two triangles filling the callback viewport. NDC y is up; the viewport covers the glow rect.
    let right = vertex_index == 1u || vertex_index == 4u || vertex_index == 5u;
    let top = vertex_index == 2u || vertex_index == 3u || vertex_index == 5u;
    var out: VsOut;
    out.position = vec4<f32>(select(-1.0, 1.0, right), select(-1.0, 1.0, top), 0.0, 1.0);
    return out;
}

fn sd_rounded_box(point: vec2<f32>, half_extent: vec2<f32>, radius: f32) -> f32 {
    let q = abs(point) - half_extent + vec2<f32>(radius, radius);
    return length(max(q, vec2<f32>(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - radius;
}

@fragment
fn fs_main(vs: VsOut) -> @location(0) vec4<f32> {
    let screen = glow.params.zw;
    if (screen.x <= 0.0 || screen.y <= 0.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    let min_px = glow.rect.xy;
    let max_px = glow.rect.zw;
    let center = (min_px + max_px) * 0.5;
    let half_extent = abs(max_px - min_px) * 0.5;
    let radius = clamp(glow.params.x, 0.0, min(half_extent.x, half_extent.y));
    let dist = sd_rounded_box(vs.position.xy - center, half_extent, radius);
    let width = max(glow.flags.y, 1.0);
    let outside = smoothstep(0.0, 1.5, dist);
    let sigma = width * 0.42;
    let falloff = exp(-0.5 * dist * dist / (sigma * sigma));
    let breathe = 0.62 + 0.38 * sin(glow.params.y * 6.28318530718 / 1.6);
    let pulse = select(1.0, breathe, glow.flags.x > 0.5);
    let alpha = glow.color.w * pulse * falloff * outside;
    return vec4<f32>(glow.color.xyz * alpha, alpha);
}

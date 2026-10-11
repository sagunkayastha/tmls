// One pipeline for everything the grid draws: instanced quads, four vertices each (a triangle
// strip), drawn in buffer order. `kind` picks the colour source:
//   0 solid   - `color` (backgrounds, cursor, underline, selection, photon square)
//   1 mask    - `color` times the atlas alpha (ordinary glyphs)
//   2 colour  - the atlas texel itself (colour emoji)
// Positions are pixels with the origin at the top-left; `uv` is an atlas rectangle in texels.
// Output is premultiplied alpha.

struct Globals {
    // Target size in pixels.
    viewport: vec2<f32>,
    // 1 when the target format is sRGB-encoded (the GPU then expects linear output).
    srgb_target: u32,
    _pad: u32,
}

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var atlas: texture_2d<f32>;

struct Instance {
    @location(0) pos: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) uv: vec4<f32>,
    @location(3) color: vec4<f32>,
    @location(4) kind: u32,
}

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) kind: u32,
}

@vertex
fn vs_main(@builtin(vertex_index) vertex: u32, instance: Instance) -> VsOut {
    let corner = vec2<f32>(f32(vertex & 1u), f32((vertex >> 1u) & 1u));
    let px = instance.pos + corner * instance.size;
    var out: VsOut;
    out.clip = vec4<f32>(px.x / globals.viewport.x * 2.0 - 1.0, 1.0 - px.y / globals.viewport.y * 2.0, 0.0, 1.0);
    out.uv = instance.uv.xy + corner * instance.uv.zw;
    out.color = instance.color;
    out.kind = instance.kind;
    return out;
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(high, low, c <= vec3<f32>(0.04045));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    var color = in.color;
    if in.kind != 0u {
        // Quads sit on whole pixels at 1:1, so every fragment centre maps to one texel.
        let texel = textureLoad(atlas, vec2<i32>(floor(in.uv)), 0);
        if in.kind == 1u {
            color.a = color.a * texel.a;
        } else {
            color = texel;
        }
    }
    var rgb = color.rgb;
    if globals.srgb_target == 1u {
        rgb = srgb_to_linear(rgb);
    }
    return vec4<f32>(rgb * color.a, color.a);
}

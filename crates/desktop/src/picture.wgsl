// The picture in the file panel: one quad (four vertices, a triangle strip) with a part of the
// picture's texture on it. Positions are pixels with the origin at the top-left, as in the
// grid's shader (`spikes/s2-wgpu/src/gfx/shader.wgsl`); `source` is in texels.
//
// Colour, as there: the target's bytes are sRGB as they are. The texture is `Rgba8UnormSrgb`,
// so a sample is linear light (and the filter mixes texels in linear light); it is encoded to
// sRGB again for a target that is not sRGB-encoded, and left linear for one that is (the GPU
// encodes then). Output is premultiplied alpha.

struct Params {
    // Where the quad is on the target: x, y, width, height in pixels.
    dest: vec4<f32>,
    // The part of the picture on it: x, y, width, height in texels.
    source: vec4<f32>,
    // Target size in pixels.
    viewport: vec2<f32>,
    // The picture's size in texels.
    size: vec2<f32>,
    // 1 when the target format is sRGB-encoded.
    srgb_target: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var picture: texture_2d<f32>;
@group(0) @binding(2) var picture_sampler: sampler;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vertex: u32) -> VsOut {
    let corner = vec2<f32>(f32(vertex & 1u), f32((vertex >> 1u) & 1u));
    let px = params.dest.xy + corner * params.dest.zw;
    var out: VsOut;
    out.clip = vec4<f32>(px.x / params.viewport.x * 2.0 - 1.0, 1.0 - px.y / params.viewport.y * 2.0, 0.0, 1.0);
    out.uv = (params.source.xy + corner * params.source.zw) / params.size;
    return out;
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let low = c * 12.92;
    let high = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055);
    return select(high, low, c <= vec3<f32>(0.0031308));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let texel = textureSample(picture, picture_sampler, in.uv);
    var rgb = texel.rgb;
    if params.srgb_target == 0u {
        rgb = linear_to_srgb(rgb);
    }
    return vec4<f32>(rgb * texel.a, texel.a);
}

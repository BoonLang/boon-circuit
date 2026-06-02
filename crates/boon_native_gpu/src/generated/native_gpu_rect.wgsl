struct RectVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
};

@group(0) @binding(0) var texture_sampler: sampler;
@group(0) @binding(1) var texture_image: texture_2d<f32>;

fn unpack_rgba8(color: u32) -> vec4<f32> {
    let r = f32(color & 255u) / 255.0;
    let g = f32((color >> 8u) & 255u) / 255.0;
    let b = f32((color >> 16u) & 255u) / 255.0;
    let a = f32((color >> 24u) & 255u) / 255.0;
    return vec4<f32>(r, g, b, a);
}

@vertex
fn vs_main(
    @location(0) position: vec2<f32>,
    @location(1) color: u32,
    @location(2) uv: vec2<f32>,
) -> RectVertex {
    var out: RectVertex;
    out.position = vec4<f32>(position, 0.0, 1.0);
    out.color = unpack_rgba8(color);
    out.uv = uv;
    return out;
}

@fragment
fn fs_main(in: RectVertex) -> @location(0) vec4<f32> {
    return textureSample(texture_image, texture_sampler, in.uv) * in.color;
}

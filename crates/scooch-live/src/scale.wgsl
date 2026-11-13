struct Out {
    size: vec2<f32>,
    spare: vec2<f32>,
}

@group(0) @binding(0) var<uniform> out: Out;
@group(0) @binding(1) var smooth_filter: sampler;
@group(0) @binding(2) var small: texture_2d<f32>;

@vertex
fn cover(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn draw(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4<f32>(textureSampleLevel(small, smooth_filter, position.xy / out.size, 0.0).rgb, 1.0);
}

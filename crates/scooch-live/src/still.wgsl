@group(0) @binding(0) var still: texture_2d<f32>;

@vertex
fn cover(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn draw(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(still));
    let pixel = clamp(vec2<i32>(position.xy), vec2<i32>(0, 0), size - vec2<i32>(1, 1));
    return vec4<f32>(textureLoad(still, pixel, 0).rgb, 1.0);
}

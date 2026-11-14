@group(0) @binding(0) var source: texture_2d<f32>;

@vertex
fn cover(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn draw(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let last = vec2<i32>(textureDimensions(source)) - vec2<i32>(1, 1);
    let base = vec2<i32>(position.xy) * 2;
    var sum = vec3<f32>(0.0);
    for (var dy = 0; dy < 2; dy++) {
        for (var dx = 0; dx < 2; dx++) {
            sum += textureLoad(source, min(base + vec2<i32>(dx, dy), last), 0).rgb;
        }
    }
    return vec4<f32>(sum * 0.25, 1.0);
}

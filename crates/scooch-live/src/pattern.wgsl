struct Frame {
    bar: f32,
    width: f32,
    height: f32,
    half: f32,
};

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var picture: texture_2d<f32>;

struct Corner {
    @builtin(position) position: vec4<f32>,
};

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> Corner {
    let x = f32((index << 1u) & 2u);
    let y = f32(index & 2u);
    var corner: Corner;
    corner.position = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    return corner;
}

@fragment
fn fragment(corner: Corner) -> @location(0) vec4<f32> {
    let pixel = corner.position.xy;
    if frame.bar >= 0.0 && abs(pixel.x - frame.bar * frame.width) < frame.half {
        return vec4<f32>(1.0, 0.0, 1.0, 1.0);
    }
    if pixel.x > frame.width - 24.0 && pixel.x < frame.width - 8.0 && pixel.y > 8.0 && pixel.y < 24.0 {
        return vec4<f32>(0.0, 1.0, 0.0, 1.0);
    }
    let size = vec2<i32>(textureDimensions(picture));
    let spot = clamp(vec2<i32>(pixel), vec2<i32>(0, 0), size - vec2<i32>(1, 1));
    return textureLoad(picture, spot, 0);
}

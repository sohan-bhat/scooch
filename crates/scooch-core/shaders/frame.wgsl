struct Frame {
    size: vec2<f32>,
    grid: vec2<f32>,
    paint: vec2<f32>,
    s: f32,
    unit: f32,
    ramp_low: f32,
    ramp_high: f32,
    reach: f32,
    grain: f32,
    grain_scale: f32,
    grain_floor: f32,
    grain_lod: f32,
    core_lift: f32,
    core_drop: f32,
    volume_lit: f32,
    volume_dark: f32,
    spare: f32,
    shadow: vec2<f32>,
    rim: vec2<f32>,
    belly: vec2<f32>,
    unused: vec2<f32>,
}

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var smooth_filter: sampler;
@group(0) @binding(2) var canvas: texture_2d<f32>;
@group(0) @binding(3) var map_a: texture_2d<f32>;
@group(0) @binding(4) var map_b: texture_2d<f32>;
@group(0) @binding(5) var field_a: texture_2d<f32>;
@group(0) @binding(6) var field_b: texture_2d<f32>;
@group(0) @binding(7) var look_a: texture_2d<f32>;
@group(0) @binding(8) var look_b: texture_2d<f32>;
@group(0) @binding(9) var pad_a: texture_2d<f32>;
@group(0) @binding(10) var pad_b: texture_2d<f32>;
@group(0) @binding(11) var paints_a: texture_2d_array<f32>;
@group(0) @binding(12) var paints_b: texture_2d_array<f32>;
@group(0) @binding(13) var ramps_a: texture_2d<f32>;
@group(0) @binding(14) var ramps_b: texture_2d<f32>;

fn quarter(p: vec2<f32>) -> vec2<f32> {
    return p / 4.0 / frame.grid;
}

fn coarse(p: vec2<f32>) -> vec2<f32> {
    return p / 16.0 / frame.paint;
}

fn field(p: vec2<f32>) -> f32 {
    let a = textureSampleLevel(field_a, smooth_filter, quarter(p), 0.0).r;
    let b = textureSampleLevel(field_b, smooth_filter, quarter(p), 0.0).r;
    return mix(a, b, frame.s);
}

fn ramp(table: texture_2d<f32>, row: f32, d: f32) -> vec4<f32> {
    let x = clamp((d - frame.ramp_low) / (frame.ramp_high - frame.ramp_low), 0.0, 1.0);
    return textureSampleLevel(table, smooth_filter, vec2<f32>((x * 511.0 + 0.5) / 512.0, (row + 0.5) / 2.0), 0.0);
}

fn over(c: vec4<f32>, color: vec3<f32>, a: f32) -> vec4<f32> {
    let k = clamp(a, 0.0, 1.0);
    return vec4<f32>(color * k + c.rgb * (1.0 - k), k + c.a * (1.0 - k));
}

fn layer(paints: texture_2d_array<f32>, p: vec2<f32>, index: i32) -> vec4<f32> {
    return textureSampleLevel(paints, smooth_filter, coarse(p), index, 0.0);
}

fn paint(looks: texture_2d<f32>, paints: texture_2d_array<f32>, table: texture_2d<f32>, p: vec2<f32>, d: vec4<f32>, grain: f32) -> vec4<f32> {
    let here = textureSampleLevel(looks, smooth_filter, quarter(p), 0.0);
    let sky = here.r;
    let belong = here.g;
    let open = here.b;
    let light = here.a * 2.0 - 1.0;
    let first = ramp(table, 0.0, d.x);
    let second = ramp(table, 1.0, d.x);
    let shade = ramp(table, 1.0, d.y);
    let shore = layer(paints, p, 0);
    let fill = layer(paints, p, 2);
    var land = vec4<f32>(0.0);
    land = over(land, vec3<f32>(1.0), first.r * shore.a);
    land = over(land, vec3<f32>(0.0), shade.b);
    land = over(land, min(shore.rgb * grain, vec3<f32>(1.0)), first.g);
    land = over(land, min(layer(paints, p, 1).rgb * grain, vec3<f32>(1.0)), first.b);
    land = over(land, min(fill.rgb * grain, vec3<f32>(1.0)), first.a);
    land = over(land, min(layer(paints, p, 3).rgb * grain, vec3<f32>(1.0)), second.r);
    land = land * fill.a;
    let core = layer(paints, p, 4);
    let rim = layer(paints, p, 6).rgb;
    let belly = layer(paints, p, 7).rgb;
    var tone = mix(core.rgb, layer(paints, p, 5).rgb, open);
    let lift = mix(frame.core_lift, frame.volume_lit, open);
    let drop = mix(frame.core_drop, frame.volume_dark, open);
    tone = mix(tone, rim, max(light, 0.0) * lift);
    tone = mix(tone, belly, max(-light, 0.0) * drop);
    tone = mix(tone, rim, 1.0 - ramp(table, 0.0, d.z).g);
    tone = mix(tone, belly, 1.0 - ramp(table, 0.0, d.w).g);
    var cloud = vec4<f32>(0.0);
    cloud = over(cloud, vec3<f32>(0.0), shade.a);
    cloud = over(cloud, min(tone * grain, vec3<f32>(1.0)), second.g);
    cloud = cloud * core.a;
    return mix(land, cloud, sky) * belong;
}

fn luminance(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

@vertex
fn cover(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn draw(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let p = position.xy;
    let pixel = vec2<i32>(p);
    let from_a = vec2<f32>(textureLoad(map_a, vec2<i32>(pixel.x, 0), 0).r, textureLoad(map_a, vec2<i32>(pixel.y, 1), 0).r);
    let from_b = vec2<f32>(textureLoad(map_b, vec2<i32>(pixel.x, 0), 0).r, textureLoad(map_b, vec2<i32>(pixel.y, 1), 0).r);
    let uv = mix(from_a, from_b, frame.s);
    var picture = textureSampleLevel(canvas, smooth_filter, uv, 0.0).rgb;
    let calm_a = textureSampleLevel(pad_a, smooth_filter, quarter(p), 0.0);
    let calm_b = textureSampleLevel(pad_b, smooth_filter, quarter(p), 0.0);
    let weight_a = calm_a.a * (1.0 - frame.s);
    let weight_b = calm_b.a * frame.s;
    let weight = weight_a + weight_b;
    if (weight > 0.0) {
        picture = mix(picture, (calm_a.rgb * weight_a + calm_b.rgb * weight_b) / weight, weight);
    }
    let d = field(p);
    if (d > frame.reach) {
        return vec4<f32>(picture, 1.0);
    }
    let u = frame.unit;
    let reads = vec4<f32>(d, field(p + frame.shadow * u), field(p + frame.rim * u), field(p + frame.belly * u));
    let mean = luminance(textureSampleLevel(canvas, smooth_filter, uv, frame.grain_lod).rgb);
    let detail = (luminance(picture) - mean) / max(3.0 * pow(mean, 2.0 / 3.0), frame.grain_floor);
    let grain = pow(1.0 + clamp(detail / frame.grain_scale, -1.0, 1.0) * frame.grain, 3.0);
    let a = paint(look_a, paints_a, ramps_a, p, reads, grain);
    let b = paint(look_b, paints_b, ramps_b, p, reads, grain);
    let land = mix(a, b, frame.s);
    return vec4<f32>(land.rgb + picture * (1.0 - land.a), 1.0);
}

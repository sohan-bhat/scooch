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

fn change(p: vec2<f32>) -> f32 {
    return abs(textureSampleLevel(field_a, smooth_filter, quarter(p), 0.0).r - textureSampleLevel(field_b, smooth_filter, quarter(p), 0.0).r);
}

fn gap(p: vec2<f32>, here: f32) -> f32 {
    let r = 0.3 * frame.unit;
    let across = max(change(p + vec2<f32>(r, 0.0)), change(p - vec2<f32>(r, 0.0)));
    let down = max(change(p + vec2<f32>(0.0, r)), change(p - vec2<f32>(0.0, r)));
    return max(here, max(across, down));
}

fn coarse(p: vec2<f32>) -> vec2<f32> {
    return p / 16.0 / frame.paint;
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

fn grain_hash(x: u32, y: u32) -> f32 {
    var h = (x * 0x8da6b343u) ^ (y * 0xd8163841u);
    h = h ^ (h >> 13u);
    h = h * 0x5bd1e995u;
    h = h ^ (h >> 15u);
    return f32(h >> 8u) / 16777215.0;
}

fn sand_grain(pixel: vec2<u32>) -> f32 {
    return 1.0 + frame.grain * ((grain_hash(pixel.x, pixel.y) - 0.5) + 0.6 * (grain_hash(pixel.x / 2u, pixel.y / 2u) - 0.5));
}

struct Parts {
    shallow: vec4<f32>,
    rest: vec4<f32>,
}

fn parts(looks: texture_2d<f32>, paints: texture_2d_array<f32>, table: texture_2d<f32>, p: vec2<f32>, d: f32, own: f32, picture: vec3<f32>, grain: f32) -> Parts {
    let here = textureSampleLevel(looks, smooth_filter, quarter(p), 0.0);
    let kind = here.r;
    let mark = here.g;
    let broad = here.b;
    let facing = here.a * 2.0 - 1.0;
    let first = ramp(table, 0.0, d);
    let second = ramp(table, 1.0, d);
    let gain = layer(paints, p, 0);
    let sand = layer(paints, p, 2);
    let keep = sand.a * (1.0 - kind);
    var shallow = vec4<f32>(0.0);
    shallow = over(shallow, min(picture * gain.rgb * 4.0, vec3<f32>(1.0)), ramp(table, 0.0, own).r * gain.a);
    var land = vec4<f32>(0.0);
    land = over(land, layer(paints, p, 1).rgb, first.g * mark);
    land = over(land, min(mix(sand.rgb, layer(paints, p, 3).rgb, first.a) * grain, vec3<f32>(1.0)), first.b);
    let shaded = layer(paints, p, 4);
    let lit = layer(paints, p, 5).rgb;
    let deep = layer(paints, p, 6).rgb;
    var tone = mix(shaded.rgb, lit, frame.volume_lit * broad);
    tone = mix(tone, deep, second.b * max(-facing, 0.0) * frame.volume_dark);
    tone = mix(tone, lit, second.g * max(facing, 0.0));
    var cloud = vec4<f32>(0.0);
    cloud = over(cloud, min(tone * (1.0 + mark * frame.core_lift - frame.core_drop), vec3<f32>(1.0)), second.r);
    return Parts(shallow * keep, land * keep + cloud * (shaded.a * kind));
}

fn under(shallow: vec4<f32>, rest: vec4<f32>) -> vec4<f32> {
    return rest + shallow * (1.0 - rest.a);
}

fn encode(c: vec3<f32>) -> vec3<f32> {
    let v = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    return select(1.055 * pow(v, vec3<f32>(1.0 / 2.4)) - 0.055, v * 12.92, v <= vec3<f32>(0.0031308));
}

fn decode(c: vec3<f32>) -> vec3<f32> {
    let v = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    return select(pow((v + 0.055) / 1.055, vec3<f32>(2.4)), v / 12.92, v <= vec3<f32>(0.04045));
}

fn over_picture(picture: vec3<f32>, land: vec4<f32>) -> vec3<f32> {
    let a = clamp(land.a, 0.0, 1.0);
    if (a <= 0.0) {
        return picture;
    }
    let straight = encode(land.rgb / a);
    return decode(straight * a + encode(picture) * (1.0 - a));
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
    let d_a = textureSampleLevel(field_a, smooth_filter, quarter(p), 0.0).r;
    let d_b = textureSampleLevel(field_b, smooth_filter, quarter(p), 0.0).r;
    if (min(d_a, d_b) > frame.reach) {
        return vec4<f32>(picture, 1.0);
    }
    let grain = sand_grain(vec2<u32>(p));
    var land = vec4<f32>(0.0);
    if (frame.spare > 0.5) {
        let a = parts(look_a, paints_a, ramps_a, p, d_a, d_a, picture, grain);
        let b = parts(look_b, paints_b, ramps_b, p, d_b, d_b, picture, grain);
        land = mix(under(a.shallow, a.rest), under(b.shallow, b.rest), frame.s);
    } else {
        let ta = d_a + frame.s * frame.grain_scale;
        let tb = d_b + (1.0 - frame.s) * frame.grain_scale;
        let kept = max(d_a, d_b) + frame.shadow.x * 4.0 * frame.s * (1.0 - frame.s) * smoothstep(0.0, frame.shadow.y, gap(p, abs(d_a - d_b)));
        let d = min(kept, min(ta, tb));
        var w = smoothstep(-frame.grain_floor, frame.grain_floor, ta - tb);
        w = w * min(4.0 * frame.s, 1.0);
        w = 1.0 - (1.0 - w) * min(4.0 * (1.0 - frame.s), 1.0);
        let a = parts(look_a, paints_a, ramps_a, p, d, d_a, picture, grain);
        let b = parts(look_b, paints_b, ramps_b, p, d, d_b, picture, grain);
        land = under(mix(a.shallow, b.shallow, frame.s), mix(a.rest, b.rest, w));
    }
    return vec4<f32>(over_picture(picture, land), 1.0);
}

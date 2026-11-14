#version 300 es
precision highp float;
precision highp int;
precision highp sampler2D;
precision highp sampler2DArray;

uniform vec2 size;
uniform vec2 grid;
uniform vec2 paint_grid;
uniform float s;
uniform float unit;
uniform float ramp_low;
uniform float ramp_high;
uniform float reach;
uniform float grain_amount;
uniform float grain_scale;
uniform float grain_floor;
uniform float grain_lod;
uniform float spare;
uniform float core_lift;
uniform float core_drop;
uniform float volume_lit;
uniform float volume_dark;
uniform vec2 shadow_at;
uniform vec2 rim_at;
uniform vec2 belly_at;

uniform sampler2D canvas;
uniform sampler2D map_a;
uniform sampler2D map_b;
uniform sampler2D field_a;
uniform sampler2D field_b;
uniform sampler2D look_a;
uniform sampler2D look_b;
uniform sampler2D pad_a;
uniform sampler2D pad_b;
uniform sampler2DArray paints_a;
uniform sampler2DArray paints_b;
uniform sampler2D ramps_a;
uniform sampler2D ramps_b;

out vec4 color;

vec2 quarter(vec2 p) {
    return p / 4.0 / grid;
}

vec2 coarse(vec2 p) {
    return p / 16.0 / paint_grid;
}

vec4 ramp(int which, float row, float d) {
    float x = clamp((d - ramp_low) / (ramp_high - ramp_low), 0.0, 1.0);
    vec2 at = vec2((x * 511.0 + 0.5) / 512.0, (row + 0.5) / 2.0);
    return which == 0 ? textureLod(ramps_a, at, 0.0) : textureLod(ramps_b, at, 0.0);
}

vec4 over(vec4 c, vec3 tint, float a) {
    float k = clamp(a, 0.0, 1.0);
    return vec4(tint * k + c.rgb * (1.0 - k), k + c.a * (1.0 - k));
}

vec4 layer(int which, vec2 p, int index) {
    vec3 at = vec3(coarse(p), float(index));
    return which == 0 ? textureLod(paints_a, at, 0.0) : textureLod(paints_b, at, 0.0);
}

float grain_hash(uint x, uint y) {
    uint h = (x * 0x8da6b343u) ^ (y * 0xd8163841u);
    h = h ^ (h >> 13u);
    h = h * 0x5bd1e995u;
    h = h ^ (h >> 15u);
    return float(h >> 8u) / 16777215.0;
}

float sand_grain(uvec2 pixel) {
    return 1.0 + grain_amount * ((grain_hash(pixel.x, pixel.y) - 0.5) + 0.6 * (grain_hash(pixel.x / 2u, pixel.y / 2u) - 0.5));
}

struct Parts {
    vec4 shallow;
    vec4 rest;
};

Parts parts(int which, vec2 p, float d, float own, vec3 picture, float grain) {
    vec4 here = which == 0 ? textureLod(look_a, quarter(p), 0.0) : textureLod(look_b, quarter(p), 0.0);
    float kind = here.r;
    float mark = here.g;
    float broad = here.b;
    float facing = here.a * 2.0 - 1.0;
    vec4 first = ramp(which, 0.0, d);
    vec4 second = ramp(which, 1.0, d);
    vec4 gain = layer(which, p, 0);
    vec4 sand = layer(which, p, 2);
    float keep = sand.a * (1.0 - kind);
    vec4 shallow = vec4(0.0);
    shallow = over(shallow, min(picture * gain.rgb * 4.0, vec3(1.0)), ramp(which, 0.0, own).r * gain.a);
    vec4 land = vec4(0.0);
    land = over(land, layer(which, p, 1).rgb, first.g * mark);
    land = over(land, min(mix(sand.rgb, layer(which, p, 3).rgb, first.a) * grain, vec3(1.0)), first.b);
    vec4 shaded = layer(which, p, 4);
    vec3 lit = layer(which, p, 5).rgb;
    vec3 deep = layer(which, p, 6).rgb;
    vec3 tone = mix(shaded.rgb, lit, volume_lit * broad);
    tone = mix(tone, deep, second.b * max(-facing, 0.0) * volume_dark);
    tone = mix(tone, lit, second.g * max(facing, 0.0));
    vec4 cloud = vec4(0.0);
    cloud = over(cloud, min(tone * (1.0 + mark * core_lift - core_drop), vec3(1.0)), second.r);
    return Parts(shallow * keep, land * keep + cloud * (shaded.a * kind));
}

vec4 under(vec4 shallow, vec4 rest) {
    return rest + shallow * (1.0 - rest.a);
}

vec3 encode(vec3 c) {
    vec3 v = clamp(c, 0.0, 1.0);
    return mix(1.055 * pow(v, vec3(1.0 / 2.4)) - 0.055, v * 12.92, lessThanEqual(v, vec3(0.0031308)));
}

vec3 decode(vec3 c) {
    vec3 v = clamp(c, 0.0, 1.0);
    return mix(pow((v + 0.055) / 1.055, vec3(2.4)), v / 12.92, lessThanEqual(v, vec3(0.04045)));
}

vec3 over_picture(vec3 picture, vec4 land) {
    float a = clamp(land.a, 0.0, 1.0);
    if (a <= 0.0) {
        return picture;
    }
    vec3 straight = encode(land.rgb / a);
    return decode(straight * a + encode(picture) * (1.0 - a));
}

void main() {
    vec2 p = vec2(gl_FragCoord.x, size.y - gl_FragCoord.y);
    ivec2 pixel = ivec2(p);
    vec2 from_a = vec2(texelFetch(map_a, ivec2(pixel.x, 0), 0).r, texelFetch(map_a, ivec2(pixel.y, 1), 0).r);
    vec2 from_b = vec2(texelFetch(map_b, ivec2(pixel.x, 0), 0).r, texelFetch(map_b, ivec2(pixel.y, 1), 0).r);
    vec2 uv = mix(from_a, from_b, s);
    vec3 picture = textureLod(canvas, uv, 0.0).rgb;
    vec4 calm_a = textureLod(pad_a, quarter(p), 0.0);
    vec4 calm_b = textureLod(pad_b, quarter(p), 0.0);
    float weight_a = calm_a.a * (1.0 - s);
    float weight_b = calm_b.a * s;
    float weight = weight_a + weight_b;
    if (weight > 0.0) {
        picture = mix(picture, (calm_a.rgb * weight_a + calm_b.rgb * weight_b) / weight, weight);
    }
    float d_a = textureLod(field_a, quarter(p), 0.0).r;
    float d_b = textureLod(field_b, quarter(p), 0.0).r;
    if (min(d_a, d_b) > reach) {
        color = vec4(picture, 1.0);
        return;
    }
    float grain = sand_grain(uvec2(p));
    vec4 land = vec4(0.0);
    if (spare > 0.5) {
        Parts a = parts(0, p, d_a, d_a, picture, grain);
        Parts b = parts(1, p, d_b, d_b, picture, grain);
        land = mix(under(a.shallow, a.rest), under(b.shallow, b.rest), s);
    } else {
        float ta = d_a + s * grain_scale;
        float tb = d_b + (1.0 - s) * grain_scale;
        float kept = max(d_a, d_b) + shadow_at.x * 4.0 * s * (1.0 - s) * smoothstep(0.0, shadow_at.y, abs(d_a - d_b));
        float d = min(kept, min(ta, tb));
        float w = smoothstep(-grain_floor, grain_floor, ta - tb);
        w = w * min(4.0 * s, 1.0);
        w = 1.0 - (1.0 - w) * min(4.0 * (1.0 - s), 1.0);
        Parts a = parts(0, p, d, d_a, picture, grain);
        Parts b = parts(1, p, d, d_b, picture, grain);
        land = under(mix(a.shallow, b.shallow, s), mix(a.rest, b.rest, w));
    }
    color = vec4(over_picture(picture, land), 1.0);
}

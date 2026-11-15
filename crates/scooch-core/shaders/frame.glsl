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

float change(vec2 p) {
    return abs(textureLod(field_a, quarter(p), 0.0).r - textureLod(field_b, quarter(p), 0.0).r);
}

float gap(vec2 p, float here) {
    float side = 0.3 * unit * 0.70710677;
    float ahead = max(change(p + vec2(side, side)), change(p + vec2(side, -side)));
    float behind = max(change(p - vec2(side, side)), change(p - vec2(side, -side)));
    return max(here, max(ahead, behind));
}

vec2 coarse(vec2 p) {
    return p / 16.0 / paint_grid;
}

float map_at(int which, ivec2 at) {
    return which == 0 ? texelFetch(map_a, at, 0).r : texelFetch(map_b, at, 0).r;
}

float seek(int which, int row, int count, float want, float start) {
    float q = start;
    for (int k = 0; k < 2; k++) {
        float g = clamp(q - 0.5, 0.0, float(count - 1));
        int i = min(int(g), max(count - 2, 0));
        float a = map_at(which, ivec2(i, row));
        float slope = map_at(which, ivec2(min(i + 1, count - 1), row)) - a;
        if (slope != 0.0) {
            q = q + (want - (a + slope * (g - float(i)))) / slope;
        }
    }
    return q;
}

vec2 follow(int which, ivec2 pixel, vec2 seen, vec2 uv, vec2 p) {
    ivec2 count = ivec2(size);
    ivec2 lo = max(pixel - ivec2(1), ivec2(0));
    ivec2 hi = min(pixel + ivec2(1), count - ivec2(1));
    vec2 rise = vec2(map_at(which, ivec2(hi.x, 0)) - map_at(which, ivec2(lo.x, 0)), map_at(which, ivec2(hi.y, 1)) - map_at(which, ivec2(lo.y, 1)));
    vec2 slope = rise / vec2(max(hi - lo, ivec2(1)));
    vec2 start = p + vec2(slope.x != 0.0 ? (uv.x - seen.x) / slope.x : 0.0, slope.y != 0.0 ? (uv.y - seen.y) / slope.y : 0.0);
    return vec2(seek(which, 0, count.x, uv.x, start.x), seek(which, 1, count.y, uv.y, start.y));
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
    vec3 body = mix(sand.rgb, layer(which, p, 3).rgb, first.a) * grain;
    float deeper = 1.0 - broad * max(0.2, 1.0 - 0.373 / max(dot(body, vec3(0.2126, 0.7152, 0.0722)), 0.0001));
    land = over(land, min(body * deeper, vec3(1.0)), first.b);
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
        float sky_a = textureLod(look_a, quarter(p), 0.0).r * (1.0 - smoothstep(0.5 * reach, reach, d_a));
        float sky_b = textureLod(look_b, quarter(p), 0.0).r * (1.0 - smoothstep(0.5 * reach, reach, d_b));
        float tide = 1.0 - max(sky_a, sky_b);
        vec4 faded = vec4(0.0);
        if (tide < 1.0) {
            Parts a = parts(0, p, d_a, d_a, picture, grain);
            Parts b = parts(1, p, d_b, d_b, picture, grain);
            faded = mix(under(a.shallow, a.rest), under(b.shallow, b.rest), s);
        }
        vec4 moved = vec4(0.0);
        if (tide > 0.0) {
            float ta = d_a + s * grain_scale;
            float tb = d_b + (1.0 - s) * grain_scale;
            float bump = 4.0 * s * (1.0 - s);
            float both = max(d_a, d_b);
            float near = min(ta, tb);
            float most = shadow_at.x * bump * bump;
            float differ = 0.0;
            if (near > both && min(near, both + most) > -0.4 && both < 0.06) {
                differ = gap(p, abs(d_a - d_b));
            }
            float kept = both + most * smoothstep(0.0, shadow_at.y, differ);
            float d = min(kept, min(ta, tb));
            float w = smoothstep(-grain_floor, grain_floor, ta - tb);
            w = w * min(4.0 * s, 1.0);
            w = 1.0 - (1.0 - w) * min(4.0 * (1.0 - s), 1.0);
            Parts a = parts(0, p, d, d_a, picture, grain);
            Parts b = parts(1, p, d, d_b, picture, grain);
            moved = under(mix(a.shallow, b.shallow, s), mix(a.rest, b.rest, w));
        }
        land = mix(faded, moved, clamp(tide, 0.0, 1.0));
    }
    float guard_a = layer(0, follow(0, pixel, from_a, uv, p), 7).a;
    float guard_b = layer(1, follow(1, pixel, from_b, uv, p), 7).a;
    float block = max(guard_a * min(4.0 * (1.0 - s), 1.0), guard_b * min(4.0 * s, 1.0));
    land = land * (1.0 - block);
    color = vec4(over_picture(picture, land), 1.0);
}

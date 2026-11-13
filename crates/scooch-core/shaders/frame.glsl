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

float field(vec2 p) {
    return mix(textureLod(field_a, quarter(p), 0.0).r, textureLod(field_b, quarter(p), 0.0).r, s);
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

vec4 paint(int which, vec2 p, vec4 d, float grain) {
    vec4 here = which == 0 ? textureLod(look_a, quarter(p), 0.0) : textureLod(look_b, quarter(p), 0.0);
    float sky = here.r;
    float belong = here.g;
    float open = here.b;
    float light = here.a * 2.0 - 1.0;
    vec4 first = ramp(which, 0.0, d.x);
    vec4 second = ramp(which, 1.0, d.x);
    vec4 shade = ramp(which, 1.0, d.y);
    vec4 shore = layer(which, p, 0);
    vec4 fill = layer(which, p, 2);
    vec4 land = vec4(0.0);
    land = over(land, vec3(1.0), first.r * shore.a);
    land = over(land, vec3(0.0), shade.b);
    land = over(land, min(shore.rgb * grain, vec3(1.0)), first.g);
    land = over(land, min(layer(which, p, 1).rgb * grain, vec3(1.0)), first.b);
    land = over(land, min(fill.rgb * grain, vec3(1.0)), first.a);
    land = over(land, min(layer(which, p, 3).rgb * grain, vec3(1.0)), second.r);
    land = land * fill.a;
    vec4 core = layer(which, p, 4);
    vec3 rim = layer(which, p, 6).rgb;
    vec3 belly = layer(which, p, 7).rgb;
    vec3 tone = mix(core.rgb, layer(which, p, 5).rgb, open);
    tone = mix(tone, rim, max(light, 0.0) * mix(core_lift, volume_lit, open));
    tone = mix(tone, belly, max(-light, 0.0) * mix(core_drop, volume_dark, open));
    tone = mix(tone, rim, 1.0 - ramp(which, 0.0, d.z).g);
    tone = mix(tone, belly, 1.0 - ramp(which, 0.0, d.w).g);
    vec4 cloud = vec4(0.0);
    cloud = over(cloud, vec3(0.0), shade.a);
    cloud = over(cloud, min(tone * grain, vec3(1.0)), second.g);
    cloud = cloud * core.a;
    return mix(land, cloud, sky) * belong;
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

float luminance(vec3 c) {
    return dot(c, vec3(0.2126, 0.7152, 0.0722));
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
    float d = field(p);
    if (d > reach) {
        color = vec4(picture, 1.0);
        return;
    }
    vec4 reads = vec4(d, field(p + shadow_at * unit), field(p + rim_at * unit), field(p + belly_at * unit));
    float mean = luminance(textureLod(canvas, uv, grain_lod).rgb);
    float detail = (luminance(picture) - mean) / max(3.0 * pow(mean, 2.0 / 3.0), grain_floor);
    float grain = pow(1.0 + clamp(detail / grain_scale, -1.0, 1.0) * grain_amount, 3.0);
    vec4 a = paint(0, p, reads, grain);
    vec4 b = paint(1, p, reads, grain);
    vec4 land = mix(a, b, s);
    color = vec4(over_picture(picture, land), 1.0);
}

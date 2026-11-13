#[path = "support/layouts.rs"]
mod layouts;

use naga::valid::{Capabilities, ValidationFlags, Validator};
use scooch_core::{Options, RgbImage, Scene, Scenery};

fn validate(module: &naga::Module, name: &str) {
    Validator::new(ValidationFlags::all(), Capabilities::all()).validate(module).unwrap_or_else(|e| panic!("{name} does not validate: {e:?}"));
}

#[test]
fn the_wgsl_frame_parses_and_validates() {
    let source = include_str!("../shaders/frame.wgsl");
    let module = naga::front::wgsl::parse_str(source).unwrap_or_else(|e| panic!("{}", e.emit_to_string(source)));
    assert!(module.entry_points.iter().any(|e| e.name == "draw"));
    validate(&module, "frame.wgsl");
}

fn vulkan(source: &str) -> String {
    let mut block = Vec::new();
    let mut body = Vec::new();
    let mut textures = Vec::new();
    for line in source.lines() {
        let words: Vec<&str> = line.trim_end_matches(';').split_whitespace().collect();
        match words.as_slice() {
            ["#version", ..] => body.push("#version 450".to_string()),
            ["precision", ..] => {}
            ["uniform", kind, name] if kind.starts_with("sampler") => {
                let binding = textures.len() + 2;
                body.push(format!("layout(set = 0, binding = {binding}) uniform {} {name}_texture;", kind.replace("sampler", "texture")));
                textures.push((kind.to_string(), name.to_string()));
            }
            ["uniform", kind, name] => block.push(format!("    {kind} {name};")),
            ["out", kind, name] => body.push(format!("layout(location = 0) out {kind} {name};")),
            _ => body.push(line.to_string()),
        }
    }
    let at = body.iter().position(|l| l.starts_with("layout(set")).unwrap_or(1);
    body.insert(at, format!("layout(set = 0, binding = 0) uniform Frame {{\n{}\n}};\nlayout(set = 0, binding = 1) uniform sampler smooth_filter;", block.join("\n")));
    let mut out = body.join("\n");
    for (kind, name) in &textures {
        for call in ["textureLod(", "texelFetch("] {
            out = out.replace(&format!("{call}{name},"), &format!("{call}{kind}({name}_texture, smooth_filter),"));
        }
    }
    out
}

#[test]
fn the_glsl_frame_parses_and_validates() {
    let source = vulkan(include_str!("../shaders/frame.glsl"));
    let mut frontend = naga::front::glsl::Frontend::default();
    let module = frontend.parse(&naga::front::glsl::Options::from(naga::ShaderStage::Fragment), &source).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
    validate(&module, "frame.glsl");
}

#[test]
fn both_shaders_read_the_same_tables() {
    let wgsl = include_str!("../shaders/frame.wgsl");
    let glsl = include_str!("../shaders/frame.glsl");
    for name in ["canvas", "map_a", "map_b", "field_a", "field_b", "look_a", "look_b", "pad_a", "pad_b", "paints_a", "paints_b", "ramps_a", "ramps_b"] {
        assert!(wgsl.contains(&format!("var {name}:")), "{name} missing in wgsl");
        assert!(glsl.contains(&format!(" {name};")), "{name} missing in glsl");
    }
}

fn encode(c: f32) -> f32 {
    let v = c.clamp(0.0, 1.0);
    if v <= 0.0031308 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
}

fn decode(c: f32) -> f32 {
    let v = c.clamp(0.0, 1.0);
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

fn over_picture(picture: [f32; 3], land: [f32; 4]) -> [f32; 3] {
    let a = land[3].clamp(0.0, 1.0);
    if a <= 0.0 {
        return picture;
    }
    let straight: [f32; 3] = std::array::from_fn(|k| encode(land[k] / a));
    std::array::from_fn(|k| decode(straight[k] * a + encode(picture[k]) * (1.0 - a)))
}

fn surface(c: f32) -> u8 {
    (encode(c) * 255.0).round() as u8
}

fn shaded(layers: &scooch_core::Layers, blend: impl Fn([f32; 3], [f32; 4]) -> [f32; 3]) -> RgbImage {
    let mut out = layers.picture.clone();
    for (p, q) in out.pixels_mut().zip(layers.scenery.pixels()) {
        let alpha = q[3] as f32 / 255.0;
        let picture = [0, 1, 2].map(|k| decode(p[k] as f32 / 255.0));
        let land = [decode(q[0] as f32 / 255.0) * alpha, decode(q[1] as f32 / 255.0) * alpha, decode(q[2] as f32 / 255.0) * alpha, alpha];
        p.0 = blend(picture, land).map(surface);
    }
    out
}

fn gaps(a: &RgbImage, b: &RgbImage) -> (f64, u8, f64) {
    let (mut sum, mut worst, mut far) = (0u64, 0u8, 0usize);
    for (p, q) in a.pixels().zip(b.pixels()) {
        let d = (0..3).map(|k| p[k].abs_diff(q[k])).max().unwrap_or(0);
        sum += (0..3).map(|k| p[k].abs_diff(q[k]) as u64).sum::<u64>();
        worst = worst.max(d);
        far += (d > 8) as usize;
    }
    (sum as f64 / a.as_raw().len() as f64, worst, far as f64 / (a.width() * a.height()) as f64)
}

#[test]
fn the_shader_composite_matches_the_still() {
    let wgsl = include_str!("../shaders/frame.wgsl");
    let glsl = include_str!("../shaders/frame.glsl");
    for line in ["let a = clamp(land.a, 0.0, 1.0);", "if (a <= 0.0) {", "return picture;", "let straight = encode(land.rgb / a);", "return decode(straight * a + encode(picture) * (1.0 - a));", "return vec4<f32>(over_picture(picture, land), 1.0);"] {
        assert!(wgsl.contains(line), "frame.wgsl lost {line}");
    }
    for line in ["float a = clamp(land.a, 0.0, 1.0);", "if (a <= 0.0) {", "return picture;", "vec3 straight = encode(land.rgb / a);", "return decode(straight * a + encode(picture) * (1.0 - a));", "color = vec4(over_picture(picture, land), 1.0);"] {
        assert!(glsl.contains(line), "frame.glsl lost {line}");
    }
    let (w, h) = (640, 400);
    let options = Options { scenery: Scenery::Auto, ..Options::default() };
    let (mut matched, mut linear) = ((0.0f64, 0u8, 0.0f64), (0.0f64, 0u8, 0.0f64));
    for path in layouts::photos() {
        let full = scooch_core::open(&path).expect("photo");
        let scale = 800.0 / full.width().max(full.height()) as f32;
        let image = image::imageops::resize(&full, (full.width() as f32 * scale) as u32, (full.height() as f32 * scale) as u32, image::imageops::FilterType::Triangle);
        let analysis = scooch_core::analyze(&image);
        for layout in ["mac", "scattered"] {
            let scene = Scene { width: w, height: h, icons: layouts::layout(layout, &analysis, w, h) };
            let mut icons = scene.icons.clone();
            icons[0].x += 2.0 * icons[0].h;
            let next = Scene { icons, ..scene.clone() };
            let first = scooch_core::solve(&analysis, &scene, &options, None);
            let second = scooch_core::solve(&analysis, &next, &options, Some(&first));
            let a = scooch_core::bundle(&image, &analysis, &first, &scene, &options);
            let b = scooch_core::bundle(&image, &analysis, &second, &next, &options);
            for s in [0.0, 0.37, 1.0] {
                let layers = scooch_core::frame_layers(&a, &b, s);
                let still = scooch_core::frame(&a, &b, s);
                let name = path.file_stem().unwrap().to_string_lossy().to_string();
                let gap = gaps(&shaded(&layers, over_picture), &still);
                assert!(gap.0 <= 0.05 && gap.1 <= 1, "{name} {layout} s {s}: mean {:.3} worst {}", gap.0, gap.1);
                matched = (matched.0.max(gap.0), matched.1.max(gap.1), matched.2.max(gap.2));
                let old = gaps(&shaded(&layers, |p, l| std::array::from_fn(|k| l[k] + p[k] * (1.0 - l[3]))), &still);
                linear = (linear.0.max(old.0), linear.1.max(old.1), linear.2.max(old.2));
            }
        }
    }
    eprintln!("srgb composite: mean {:.4} worst {} far {:.5}; linear composite: mean {:.4} worst {} far {:.5}", matched.0, matched.1, matched.2, linear.0, linear.1, linear.2);
    assert!(linear.1 > 8, "the linear composite should differ from the still");
}

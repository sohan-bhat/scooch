use naga::valid::{Capabilities, ValidationFlags, Validator};

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

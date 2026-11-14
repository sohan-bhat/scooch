use crate::{surface_target, Gpu, Target};
use scooch_core::{Bundle, Canvas, Constants, RgbImage};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const FRAME: &str = include_str!("../../scooch-core/shaders/frame.wgsl");
const STILL: &str = include_str!("still.wgsl");
const SCALE: &str = include_str!("scale.wgsl");
pub const SCALES: [f32; 4] = [1.0, 0.75, 0.6, 0.5];
const BUDGET: f64 = 0.6;
const ROOM: f64 = 0.4;
pub const OUTPUT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;
const LAYERS: u32 = 8;
const RAMP: u32 = 512;

pub(crate) struct Pipes {
    sampler: wgpu::Sampler,
    frame_layout: wgpu::BindGroupLayout,
    frame: wgpu::RenderPipeline,
    still_layout: wgpu::BindGroupLayout,
    still: wgpu::RenderPipeline,
    scale_layout: wgpu::BindGroupLayout,
    scale: wgpu::RenderPipeline,
}

pub(crate) struct Small {
    key: (u32, u32, u32, u32),
    view: wgpu::TextureView,
    bind: wgpu::BindGroup,
}

fn texture_entry(binding: u32, filterable: bool, dimension: wgpu::TextureViewDimension) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable }, view_dimension: dimension, multisampled: false },
        count: None,
    }
}

fn blended() -> wgpu::BlendState {
    let component = wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Constant, dst_factor: wgpu::BlendFactor::OneMinusConstant, operation: wgpu::BlendOperation::Add };
    wgpu::BlendState { color: component, alpha: component }
}

fn pipeline(device: &wgpu::Device, label: &str, source: &str, vertex: &str, fragment: &str, layout: &wgpu::BindGroupLayout) -> wgpu::RenderPipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(label), source: wgpu::ShaderSource::Wgsl(source.into()) });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some(label), bind_group_layouts: &[Some(layout)], immediate_size: 0 });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState { module: &module, entry_point: Some(vertex), compilation_options: wgpu::PipelineCompilationOptions::default(), buffers: &[] },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some(fragment),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState { format: OUTPUT, blend: Some(blended()), write_mask: wgpu::ColorWrites::ALL })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

pub(crate) fn pipes(device: &wgpu::Device) -> Result<Pipes, String> {
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("smooth"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        ..Default::default()
    });
    let flat = wgpu::TextureViewDimension::D2;
    let mut entries = vec![
        wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        },
        wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
        texture_entry(2, true, flat),
        texture_entry(3, false, flat),
        texture_entry(4, false, flat),
    ];
    for binding in 5..=10 {
        entries.push(texture_entry(binding, true, flat));
    }
    entries.push(texture_entry(11, true, wgpu::TextureViewDimension::D2Array));
    entries.push(texture_entry(12, true, wgpu::TextureViewDimension::D2Array));
    entries.push(texture_entry(13, true, flat));
    entries.push(texture_entry(14, true, flat));
    let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("frame"), entries: &entries });
    let frame = pipeline(device, "frame", FRAME, "cover", "draw", &frame_layout);
    let still_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("still"), entries: &[texture_entry(0, false, flat)] });
    let still = pipeline(device, "still", STILL, "cover", "draw", &still_layout);
    let scale_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("scale"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            },
            wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            texture_entry(2, true, flat),
        ],
    });
    let scale = pipeline(device, "scale", SCALE, "cover", "draw", &scale_layout);
    if let Some(error) = pollster::block_on(scope.pop()) {
        return Err(format!("the frame shaders do not build {error}"));
    }
    Ok(Pipes { sampler, frame_layout, frame, still_layout, still, scale_layout, scale })
}

#[derive(Clone)]
pub struct Picture {
    view: wgpu::TextureView,
    pub width: u32,
    pub height: u32,
    pub levels: u32,
}

#[derive(Clone)]
pub struct Still {
    bind: wgpu::BindGroup,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone)]
struct Pair {
    bind: wgpu::BindGroup,
    uniform: wgpu::Buffer,
    values: [f32; 28],
}

#[derive(Clone)]
struct Set {
    ab: Pair,
    aa: Pair,
    bb: Pair,
}

#[derive(Clone)]
struct Shape {
    width: u32,
    height: u32,
    grid: (u32, u32),
    paint: (u32, u32),
    canvas_width: u32,
    xmap: Vec<f64>,
    ymap: Vec<f64>,
    constants: Constants,
}

impl Shape {
    fn of(bundle: &Bundle) -> Shape {
        Shape {
            width: bundle.width.max(1),
            height: bundle.height.max(1),
            grid: (bundle.grid_width, bundle.grid_height),
            paint: (bundle.paint_width, bundle.paint_height),
            canvas_width: bundle.canvas_width,
            xmap: bundle.xmap.clone(),
            ymap: bundle.ymap.clone(),
            constants: bundle.constants,
        }
    }
}

pub struct Glide {
    compatible: bool,
    full: Set,
    picture: Picture,
    tables: (Tables, Tables),
    shapes: (Shape, Shape),
    scaled: Mutex<Vec<(u32, Set)>>,
    pub width: u32,
    pub height: u32,
}

pub fn scaled_size(width: u32, height: u32, scale: f32) -> (u32, u32) {
    (((width as f32 * scale).round() as u32).max(1), ((height as f32 * scale).round() as u32).max(1))
}

fn scale_key(scale: f32) -> u32 {
    (scale.clamp(0.0, 1.0) * 1000.0).round() as u32
}

pub fn choose(frame: Duration, period: Duration) -> f32 {
    SCALES.iter().copied().find(|scale| frame.as_secs_f64() * f64::from(scale * scale) <= BUDGET * period.as_secs_f64()).unwrap_or(SCALES[SCALES.len() - 1])
}

pub fn step(scale: f32, frames: u32, slow: u32, frame: Duration, period: Duration) -> f32 {
    let index = SCALES.iter().position(|known| (known - scale).abs() < 0.01).unwrap_or(0);
    if slow * 10 > frames {
        SCALES[(index + 1).min(SCALES.len() - 1)]
    } else if frame.as_secs_f64() < ROOM * period.as_secs_f64() {
        SCALES[index.saturating_sub(1)]
    } else {
        SCALES[index]
    }
}

fn resample(map: &[f64], from: u32, to: u32) -> Vec<f64> {
    if map.is_empty() || from == to {
        return map.to_vec();
    }
    let ratio = f64::from(from) / f64::from(to);
    (0..to)
        .map(|i| {
            let at = ((f64::from(i) + 0.5) * ratio - 0.5).clamp(0.0, (map.len() - 1) as f64);
            let low = at.floor() as usize;
            let high = (low + 1).min(map.len() - 1);
            map[low] + (map[high] - map[low]) * (at - low as f64)
        })
        .collect()
}

pub fn heavy(width: u32, height: u32, picture: &Picture) -> Bundle {
    let (grid, paint) = ((width.div_ceil(4), height.div_ceil(4)), (width.div_ceil(16), height.div_ceil(16)));
    let cells = (grid.0 * grid.1) as usize;
    Bundle {
        width,
        height,
        canvas_width: picture.width,
        canvas_height: picture.height,
        canvas: Vec::new(),
        xmap: (0..width).map(|x| (f64::from(x) + 0.5) / f64::from(width)).collect(),
        ymap: (0..height).map(|y| (f64::from(y) + 0.5) / f64::from(height)).collect(),
        ratio: f64::from(picture.width) / f64::from(width.max(1)),
        grid_width: grid.0,
        grid_height: grid.1,
        field: (0..cells).map(|cell| if (cell as u32 % grid.0) * 5 < grid.0 { -0.05 } else { 1.0 }).collect(),
        look: [128u8, 255, 128, 128].repeat(cells),
        pad: (0..cells).flat_map(|cell| if (cell as u32 % grid.0) * 5 < grid.0 { [128u8, 128, 128, 128] } else { [0u8; 4] }).collect(),
        paint_width: paint.0,
        paint_height: paint.1,
        paints: [128u8, 128, 128, 255].repeat((paint.0 * paint.1 * LAYERS) as usize),
        ramps: [128u8, 128, 128, 128].repeat((RAMP * 2) as usize),
        constants: Constants {
            unit: height as f32 / 10.0,
            ramp_low: -0.8,
            ramp_high: 0.8,
            reach: 0.38,
            shadow: [0.0, -0.1],
            rim: [-0.1, -0.1],
            belly: [0.0, 0.14],
            grain: 0.5,
            grain_scale: 0.1,
            grain_floor: 0.03,
            core_lift: 0.08,
            core_drop: 0.15,
            volume_lit: 0.6,
            volume_dark: 0.5,
        },
    }
}

pub enum Look<'a> {
    Still(&'a Still),
    Glide(&'a Glide, f32),
}

#[derive(Clone)]
struct Tables {
    map: wgpu::TextureView,
    field: wgpu::TextureView,
    look: wgpu::TextureView,
    pad: wgpu::TextureView,
    paints: wgpu::TextureView,
    ramps: wgpu::TextureView,
}

pub fn half(v: f32) -> u16 {
    let bits = v.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    if v.is_nan() || v == 0.0 {
        return sign;
    }
    let magnitude = v.abs();
    if magnitude >= 65520.0 {
        return sign | 0x7bff;
    }
    if magnitude < 6.103_515_6e-5 {
        return sign | (magnitude / 5.960_464_5e-8).round_ties_even() as u16;
    }
    let exponent = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    let mantissa = bits & 0x7f_ffff;
    let mut out = ((exponent as u32) << 10) | (mantissa >> 13);
    let rest = mantissa & 0x1fff;
    if rest > 0x1000 || (rest == 0x1000 && out & 1 == 1) {
        out += 1;
    }
    sign | out.min(0x7bff) as u16
}

fn linear_table() -> [f32; 256] {
    std::array::from_fn(|i| {
        let c = i as f64 / 255.0;
        (if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }) as f32
    })
}

fn encode(v: f32) -> u8 {
    let v = v.clamp(0.0, 1.0) as f64;
    let c = if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
    (c * 255.0 + 0.5).clamp(0.0, 255.0) as u8
}

fn smaller(level: &[u8], width: u32, height: u32, decode: &[f32; 256]) -> (Vec<u8>, u32, u32) {
    let (w, h) = ((width / 2).max(1), (height / 2).max(1));
    let mut out = vec![255u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0.0f32; 3];
            let mut count = 0.0f32;
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let (sx, sy) = ((x * 2 + dx).min(width - 1), (y * 2 + dy).min(height - 1));
                let at = ((sy * width + sx) * 4) as usize;
                for (channel, total) in sum.iter_mut().enumerate() {
                    *total += decode[level[at + channel] as usize];
                }
                count += 1.0;
            }
            let at = ((y * w + x) * 4) as usize;
            for (channel, total) in sum.iter().enumerate() {
                out[at + channel] = encode(total / count);
            }
        }
    }
    (out, w, h)
}

fn rgba(rgb: &[u8]) -> Vec<u8> {
    rgb.as_chunks::<3>().0.iter().flat_map(|pixel| [pixel[0], pixel[1], pixel[2], 255]).collect()
}

impl Gpu {
    fn texture(&self, label: &str, size: (u32, u32, u32), format: wgpu::TextureFormat, levels: u32) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: size.2 },
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    }

    fn write(&self, texture: &wgpu::Texture, level: u32, size: (u32, u32, u32), bytes_per_pixel: u32, data: &[u8]) {
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture, mip_level: level, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            data,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(size.0 * bytes_per_pixel), rows_per_image: Some(size.1) },
            wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: size.2 },
        );
    }

    fn view(texture: &wgpu::Texture, dimension: wgpu::TextureViewDimension) -> wgpu::TextureView {
        texture.create_view(&wgpu::TextureViewDescriptor { dimension: Some(dimension), ..Default::default() })
    }

    pub(crate) fn pipes(&self) -> Result<&Pipes, String> {
        self.scenes.as_ref().as_ref().map_err(Clone::clone)
    }

    pub fn ready(&self) -> Result<(), String> {
        self.pipes().map(|_| ())
    }

    pub fn largest(&self) -> u32 {
        self.device.limits().max_texture_dimension_2d
    }

    pub fn fits(&self, width: u32, height: u32) -> Result<(), String> {
        let most = self.largest();
        if width > most || height > most {
            return Err(format!("the screen is too large for this GPU, {width}x{height} against {most}"));
        }
        Ok(())
    }

    pub fn picture(&self, canvas: &Canvas) -> Picture {
        self.try_picture(canvas).unwrap_or_else(|_| {
            let texture = self.texture("canvas", (1, 1, 1), wgpu::TextureFormat::Rgba8UnormSrgb, 1);
            Picture { view: Self::view(&texture, wgpu::TextureViewDimension::D2), width: 0, height: 0, levels: 1 }
        })
    }

    pub fn try_picture(&self, canvas: &Canvas) -> Result<Picture, String> {
        let (width, height) = (canvas.width.max(1), canvas.height.max(1));
        self.fits(width, height)?;
        let levels = 32 - width.max(height).leading_zeros();
        let texture = self.texture("canvas", (width, height, 1), wgpu::TextureFormat::Rgba8UnormSrgb, levels);
        let decode = linear_table();
        let mut level = rgba(&canvas.pixels);
        let (mut w, mut h) = (width, height);
        for index in 0..levels {
            self.write(&texture, index, (w, h, 1), 4, &level);
            if index + 1 < levels {
                (level, w, h) = smaller(&level, w, h, &decode);
            }
        }
        Ok(Picture { view: Self::view(&texture, wgpu::TextureViewDimension::D2), width, height, levels })
    }

    pub fn still(&self, image: &RgbImage) -> Result<Still, String> {
        let pipes = self.pipes()?;
        let (width, height) = (image.width().max(1), image.height().max(1));
        self.fits(width, height)?;
        let texture = self.texture("still", (width, height, 1), wgpu::TextureFormat::Rgba8UnormSrgb, 1);
        self.write(&texture, 0, (width, height, 1), 4, &rgba(image.as_raw()));
        let view = Self::view(&texture, wgpu::TextureViewDimension::D2);
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("still"), layout: &pipes.still_layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) }] });
        Ok(Still { bind, width, height })
    }

    fn map(&self, xmap: &[f64], ymap: &[f64], width: u32, height: u32) -> wgpu::TextureView {
        let long = width.max(height).max(1);
        let row = |values: &[f64]| -> Vec<f32> {
            let last = values.last().copied().unwrap_or(0.0) as f32;
            (0..long as usize).map(|i| values.get(i).map_or(last, |v| *v as f32)).collect()
        };
        let map_values: Vec<u8> = row(xmap).into_iter().chain(row(ymap)).flat_map(f32::to_le_bytes).collect();
        let map = self.texture("map", (long, 2, 1), wgpu::TextureFormat::R32Float, 1);
        self.write(&map, 0, (long, 2, 1), 4, &map_values);
        Self::view(&map, wgpu::TextureViewDimension::D2)
    }

    fn tables(&self, bundle: &Bundle) -> Tables {
        let map = self.map(&bundle.xmap, &bundle.ymap, bundle.width.max(1), bundle.height.max(1));
        let grid = (bundle.grid_width.max(1), bundle.grid_height.max(1), 1);
        let cells = (grid.0 * grid.1) as usize;
        let field_values: Vec<u8> = (0..cells).flat_map(|i| half(bundle.field.get(i).copied().unwrap_or(1.0)).to_le_bytes()).collect();
        let field = self.texture("field", grid, wgpu::TextureFormat::R16Float, 1);
        self.write(&field, 0, grid, 2, &field_values);
        let sized = |bytes: &[u8], count: usize| -> Vec<u8> {
            let mut out = bytes.to_vec();
            out.resize(count, 0);
            out
        };
        let look = self.texture("look", grid, wgpu::TextureFormat::Rgba8Unorm, 1);
        self.write(&look, 0, grid, 4, &sized(&bundle.look, cells * 4));
        let pad = self.texture("pad", grid, wgpu::TextureFormat::Rgba8UnormSrgb, 1);
        self.write(&pad, 0, grid, 4, &sized(&bundle.pad, cells * 4));
        let coarse = (bundle.paint_width.max(1), bundle.paint_height.max(1), LAYERS);
        let paints = self.texture("paints", coarse, wgpu::TextureFormat::Rgba8UnormSrgb, 1);
        self.write(&paints, 0, coarse, 4, &sized(&bundle.paints, (coarse.0 * coarse.1 * LAYERS * 4) as usize));
        let ramps = self.texture("ramps", (RAMP, 2, 1), wgpu::TextureFormat::Rgba8Unorm, 1);
        self.write(&ramps, 0, (RAMP, 2, 1), 4, &sized(&bundle.ramps, (RAMP * 2 * 4) as usize));
        let flat = wgpu::TextureViewDimension::D2;
        Tables {
            map,
            field: Self::view(&field, flat),
            look: Self::view(&look, flat),
            pad: Self::view(&pad, flat),
            paints: Self::view(&paints, wgpu::TextureViewDimension::D2Array),
            ramps: Self::view(&ramps, flat),
        }
    }

    fn pair(&self, pipes: &Pipes, picture: &Picture, first: (&Shape, &Tables), second: &Tables, size: (u32, u32)) -> Pair {
        let (shape, a) = first;
        let b = second;
        let c = shape.constants;
        let (rx, ry) = (size.0 as f32 / shape.width as f32, size.1 as f32 / shape.height as f32);
        let lod = (4.0 * shape.canvas_width.max(1) as f32 / size.0.max(1) as f32).log2();
        let values = [
            size.0 as f32,
            size.1 as f32,
            shape.grid.0 as f32 * rx,
            shape.grid.1 as f32 * ry,
            shape.paint.0 as f32 * rx,
            shape.paint.1 as f32 * ry,
            0.0,
            c.unit * rx,
            c.ramp_low,
            c.ramp_high,
            c.reach,
            c.grain,
            c.grain_scale,
            c.grain_floor,
            lod,
            c.core_lift,
            c.core_drop,
            c.volume_lit,
            c.volume_dark,
            0.0,
            c.shadow[0],
            c.shadow[1],
            c.rim[0],
            c.rim[1],
            c.belly[0],
            c.belly[1],
            0.0,
            0.0,
        ];
        let uniform = self.device.create_buffer(&wgpu::BufferDescriptor { label: Some("frame"), size: 112, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let views = [&picture.view, &a.map, &b.map, &a.field, &b.field, &a.look, &b.look, &a.pad, &b.pad, &a.paints, &b.paints, &a.ramps, &b.ramps];
        let mut entries = vec![wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&pipes.sampler) }];
        for (index, view) in views.iter().enumerate() {
            entries.push(wgpu::BindGroupEntry { binding: index as u32 + 2, resource: wgpu::BindingResource::TextureView(view) });
        }
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("frame"), layout: &pipes.frame_layout, entries: &entries });
        Pair { bind, uniform, values }
    }

    pub fn glide(&self, picture: &Picture, a: &Bundle, b: &Bundle) -> Result<Glide, String> {
        let pipes = self.pipes()?;
        if a.canvas_width != picture.width || a.canvas_height != picture.height || b.canvas_width != picture.width || b.canvas_height != picture.height {
            return Err("the tables were made for another picture".into());
        }
        if a.width != b.width || a.height != b.height {
            return Err("the two tables are for screens of different sizes".into());
        }
        self.fits(a.width.max(a.height), 2)?;
        let (first, second) = (self.tables(a), self.tables(b));
        let shapes = (Shape::of(a), Shape::of(b));
        let size = (shapes.0.width, shapes.0.height);
        let full = Set {
            ab: self.pair(pipes, picture, (&shapes.0, &first), &second, size),
            aa: self.pair(pipes, picture, (&shapes.0, &first), &first, size),
            bb: self.pair(pipes, picture, (&shapes.1, &second), &second, size),
        };
        Ok(Glide { compatible: scooch_core::compatible(a, b), full, picture: picture.clone(), tables: (first, second), shapes, scaled: Mutex::new(Vec::new()), width: a.width, height: a.height })
    }

    fn set_at(&self, pipes: &Pipes, glide: &Glide, scale: f32) -> Set {
        let key = scale_key(scale);
        if key >= 1000 {
            return glide.full.clone();
        }
        if let Some(found) = glide.scaled.lock().ok().and_then(|scaled| scaled.iter().find(|(seen, _)| *seen == key).map(|(_, set)| set.clone())) {
            return found;
        }
        let size = scaled_size(glide.width, glide.height, scale);
        let shrink = |shape: &Shape, tables: &Tables| -> Tables {
            let map = self.map(&resample(&shape.xmap, shape.width, size.0), &resample(&shape.ymap, shape.height, size.1), size.0, size.1);
            Tables { map, ..tables.clone() }
        };
        let (first, second) = (shrink(&glide.shapes.0, &glide.tables.0), shrink(&glide.shapes.1, &glide.tables.1));
        let set = Set {
            ab: self.pair(pipes, &glide.picture, (&glide.shapes.0, &first), &second, size),
            aa: self.pair(pipes, &glide.picture, (&glide.shapes.0, &first), &first, size),
            bb: self.pair(pipes, &glide.picture, (&glide.shapes.1, &second), &second, size),
        };
        if let Ok(mut scaled) = glide.scaled.lock() {
            scaled.push((key, set.clone()));
        }
        set
    }

    fn draw_glide(&self, pass: &mut wgpu::RenderPass<'_>, pipes: &Pipes, set: &Set, compatible: bool, s: f32, k: f32) {
        let s = if s.is_finite() { s.clamp(0.0, 1.0) } else { 0.0 };
        if compatible {
            self.draw_pair(pass, pipes, &set.ab, s, k);
        } else if s <= 0.0 {
            self.draw_pair(pass, pipes, &set.aa, 0.0, k);
        } else if s >= 1.0 {
            self.draw_pair(pass, pipes, &set.bb, 0.0, k);
        } else {
            self.draw_pair(pass, pipes, &set.aa, 0.0, k);
            self.draw_pair(pass, pipes, &set.bb, 0.0, s * k);
        }
    }

    fn small(&self, pipes: &Pipes, output: (u32, u32), size: (u32, u32)) -> (wgpu::TextureView, wgpu::BindGroup) {
        let key = (output.0, output.1, size.0, size.1);
        if let Some(found) = self.small.lock().ok().and_then(|small| small.as_ref().filter(|small| small.key == key).map(|small| (small.view.clone(), small.bind.clone()))) {
            return found;
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("small"),
            size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: OUTPUT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let uniform = self.device.create_buffer(&wgpu::BufferDescriptor { label: Some("scale"), size: 16, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let values: Vec<u8> = [output.0 as f32, output.1 as f32, 0.0, 0.0].iter().flat_map(|value| value.to_le_bytes()).collect();
        self.queue.write_buffer(&uniform, 0, &values);
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scale"),
            layout: &pipes.scale_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&pipes.sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&view) },
            ],
        });
        if let Ok(mut small) = self.small.lock() {
            *small = Some(Small { key, view: view.clone(), bind: bind.clone() });
        }
        (view, bind)
    }

    fn draw_pair(&self, pass: &mut wgpu::RenderPass<'_>, pipes: &Pipes, pair: &Pair, s: f32, k: f32) {
        let mut values = pair.values;
        values[6] = s;
        let bytes: Vec<u8> = values.iter().flat_map(|value| value.to_le_bytes()).collect();
        self.queue.write_buffer(&pair.uniform, 0, &bytes);
        let k = f64::from(k.clamp(0.0, 1.0));
        pass.set_pipeline(&pipes.frame);
        pass.set_blend_constant(wgpu::Color { r: k, g: k, b: k, a: k });
        pass.set_bind_group(0, &pair.bind, &[]);
        pass.draw(0..3, 0..1);
    }

    fn pass<'a>(encoder: &'a mut wgpu::CommandEncoder, view: &'a wgpu::TextureView) -> wgpu::RenderPass<'a> {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("show"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        })
    }

    fn encode(&self, view: &wgpu::TextureView, output: (u32, u32), layers: &[(Look, f32)], scale: f32) -> Result<wgpu::CommandEncoder, String> {
        let pipes = self.pipes()?;
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("show") });
        let first = layers.iter().position(|(look, _)| matches!(look, Look::Glide(..)));
        let small = match (first, scale_key(scale) < 1000) {
            (Some(index), true) => {
                let Look::Glide(glide, s) = &layers[index].0 else { return Err("no glide to scale".into()) };
                let size = scaled_size(output.0, output.1, scale);
                let set = self.set_at(pipes, glide, scale);
                let (target, bind) = self.small(pipes, output, size);
                {
                    let mut pass = Self::pass(&mut encoder, &target);
                    self.draw_glide(&mut pass, pipes, &set, glide.compatible, *s, 1.0);
                }
                Some((index, bind))
            }
            _ => None,
        };
        {
            let mut pass = Self::pass(&mut encoder, view);
            for (index, (look, k)) in layers.iter().enumerate() {
                let weight = f64::from(k.clamp(0.0, 1.0));
                match look {
                    Look::Still(still) => {
                        pass.set_pipeline(&pipes.still);
                        pass.set_blend_constant(wgpu::Color { r: weight, g: weight, b: weight, a: weight });
                        pass.set_bind_group(0, &still.bind, &[]);
                        pass.draw(0..3, 0..1);
                    }
                    Look::Glide(_, _) if small.as_ref().is_some_and(|(at, _)| *at == index) => {
                        let bind = &small.as_ref().map(|(_, bind)| bind.clone()).ok_or("no small target")?;
                        pass.set_pipeline(&pipes.scale);
                        pass.set_blend_constant(wgpu::Color { r: weight, g: weight, b: weight, a: weight });
                        pass.set_bind_group(0, bind, &[]);
                        pass.draw(0..3, 0..1);
                    }
                    Look::Glide(glide, s) => self.draw_glide(&mut pass, pipes, &glide.full, glide.compatible, *s, *k),
                }
            }
        }
        Ok(encoder)
    }

    fn target(&self, width: u32, height: u32) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen"),
            size: wgpu::Extent3d { width: width.max(1), height: height.max(1), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: OUTPUT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
    }

    pub fn time(&self, width: u32, height: u32, layers: &[(Look, f32)]) -> Result<Duration, String> {
        self.time_at(width, height, layers, 1.0)
    }

    pub fn time_at(&self, width: u32, height: u32, layers: &[(Look, f32)], scale: f32) -> Result<Duration, String> {
        self.fits(width, height)?;
        let target = self.target(width, height);
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        self.device.poll(wgpu::PollType::wait_indefinitely()).map_err(|error| format!("the device did not go idle {error}"))?;
        let started = std::time::Instant::now();
        let encoder = self.encode(&view, (width, height), layers, scale)?;
        self.queue.submit([encoder.finish()]);
        self.device.poll(wgpu::PollType::wait_indefinitely()).map_err(|error| format!("the frame did not finish {error}"))?;
        Ok(started.elapsed())
    }

    pub fn snapshot(&self, width: u32, height: u32, layers: &[(Look, f32)]) -> Result<RgbImage, String> {
        self.snapshot_at(width, height, layers, 1.0)
    }

    pub fn snapshot_at(&self, width: u32, height: u32, layers: &[(Look, f32)], scale: f32) -> Result<RgbImage, String> {
        self.fits(width, height)?;
        let target = self.target(width, height);
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.encode(&view, (width, height), layers, scale)?;
        let row = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor { label: Some("readback"), size: u64::from(row) * u64::from(height), usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &target, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer: &buffer, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(height) } },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        self.queue.submit([encoder.finish()]);
        let done = Arc::new(Mutex::new(None));
        let told = done.clone();
        buffer.map_async(wgpu::MapMode::Read, .., move |result| {
            if let Ok(mut told) = told.lock() {
                *told = Some(result.is_ok());
            }
        });
        self.device.poll(wgpu::PollType::wait_indefinitely()).map_err(|error| format!("the readback did not finish {error}"))?;
        if done.lock().ok().and_then(|done| *done) != Some(true) {
            return Err("the readback could not be mapped".into());
        }
        let mapped = buffer.get_mapped_range(..).map_err(|error| format!("the readback could not be read {error}"))?;
        let mut out = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            let line = &mapped[(y * row) as usize..(y * row + width * 4) as usize];
            for pixel in line.as_chunks::<4>().0.iter() {
                out.extend_from_slice(&[pixel[2], pixel[1], pixel[0]]);
            }
        }
        drop(mapped);
        buffer.unmap();
        RgbImage::from_raw(width, height, out).ok_or_else(|| "the readback has the wrong size".to_string())
    }

    pub fn screen(&self, target: Target, width: u32, height: u32) -> Result<Screen, String> {
        self.pipes()?;
        self.fits(width, height)?;
        let surface = unsafe { self.instance.create_surface_unsafe(surface_target(&target)?) }.map_err(|error| format!("create surface {error}"))?;
        if !self.adapter.is_surface_supported(&surface) {
            return Err(format!("{} cannot present to this surface", self.name));
        }
        let capabilities = surface.get_capabilities(&self.adapter);
        let mut config = surface.get_default_config(&self.adapter, width.max(1), height.max(1)).ok_or("the surface has no default configuration")?;
        if capabilities.formats.contains(&OUTPUT) {
            config.format = OUTPUT;
        } else if capabilities.formats.contains(&OUTPUT.remove_srgb_suffix()) {
            config.format = OUTPUT.remove_srgb_suffix();
            config.view_formats = vec![OUTPUT];
        } else {
            return Err(format!("the surface offers no {OUTPUT:?}"));
        }
        config.present_mode = wgpu::PresentMode::Fifo;
        if capabilities.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
            config.alpha_mode = wgpu::CompositeAlphaMode::Opaque;
        }
        surface.configure(&self.device, &config);
        Ok(Screen { gpu: self.clone(), surface, width: config.width, height: config.height, scale: 1.0 })
    }
}

pub struct Screen {
    gpu: Gpu,
    surface: wgpu::Surface<'static>,
    pub width: u32,
    pub height: u32,
    pub scale: f32,
}

impl Screen {
    pub fn show(&mut self, layers: &[(Look, f32)]) -> Result<(), String> {
        let texture = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => texture,
            wgpu::CurrentSurfaceTexture::Timeout => return Err("the surface timed out".into()),
            wgpu::CurrentSurfaceTexture::Occluded => return Err("the surface is occluded".into()),
            wgpu::CurrentSurfaceTexture::Outdated => return Err("the surface is outdated".into()),
            wgpu::CurrentSurfaceTexture::Lost => return Err("the surface was lost".into()),
            wgpu::CurrentSurfaceTexture::Validation => return Err("the surface failed validation".into()),
        };
        let view = texture.texture.create_view(&wgpu::TextureViewDescriptor { format: Some(OUTPUT), ..Default::default() });
        let encoder = self.gpu.encode(&view, (self.width, self.height), layers, self.scale)?;
        self.gpu.queue.submit([encoder.finish()]);
        self.gpu.queue.present(texture);
        Ok(())
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        let _ = self.gpu.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: Some(std::time::Duration::from_secs(1)) });
    }
}

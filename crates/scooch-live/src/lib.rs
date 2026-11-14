#![cfg(any(windows, target_os = "macos"))]

mod glide;

pub use glide::{choose, half, heavy, scaled_size, step, Glide, Look, Picture, Screen, Still, OUTPUT, SCALES};
use std::num::NonZeroIsize;
use std::sync::{Arc, Mutex};

const SHADER: &str = include_str!("pattern.wgsl");
const BAR_HALF_WIDTH: f32 = 20.0;

pub enum Target {
    Window { window: isize, instance: isize, composition: bool },
    Layer(*mut std::ffi::c_void),
}

pub struct Pattern {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone)]
pub struct Gpu {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    composition: bool,
    pipelines: Arc<Mutex<Vec<Pipeline>>>,
    scenes: Arc<Result<glide::Pipes, String>>,
    small: Arc<Mutex<Option<glide::Small>>>,
    pub name: String,
    pub kind: &'static str,
}

#[derive(Clone)]
struct Pipeline {
    format: wgpu::TextureFormat,
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
}

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    pipeline: wgpu::RenderPipeline,
    bind: wgpu::BindGroup,
    uniform: wgpu::Buffer,
    width: u32,
    height: u32,
    pub adapter: String,
}

static MADE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub fn devices_made() -> usize {
    MADE.load(std::sync::atomic::Ordering::SeqCst)
}

fn composition(target: &Target) -> bool {
    matches!(target, Target::Window { composition: true, .. })
}

impl Gpu {
    pub fn new(composition: bool) -> Result<Self, String> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = if cfg!(windows) { wgpu::Backends::DX12 } else { wgpu::Backends::METAL };
        #[cfg(windows)]
        if composition {
            descriptor.backend_options.dx12.presentation_system = wgpu::Dx12SwapchainKind::DxgiFromVisual;
        }
        let instance = wgpu::Instance::new(descriptor);
        let mut options = wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::LowPower, ..Default::default() };
        let adapter = match pollster::block_on(instance.request_adapter(&options)) {
            Ok(adapter) => adapter,
            Err(_) => {
                options.force_fallback_adapter = true;
                pollster::block_on(instance.request_adapter(&options)).map_err(|error| format!("request adapter {error}"))?
            }
        };
        let info = adapter.get_info();
        let name = format!("{} {:?} {:?}", info.name, info.device_type, info.backend);
        let kind = match info.device_type {
            wgpu::DeviceType::Cpu => "cpu",
            wgpu::DeviceType::IntegratedGpu => "integrated",
            wgpu::DeviceType::DiscreteGpu => "discrete",
            wgpu::DeviceType::VirtualGpu => "virtual",
            wgpu::DeviceType::Other => "other",
        };
        let descriptor = wgpu::DeviceDescriptor { required_limits: adapter.limits(), ..Default::default() };
        let (device, queue) = pollster::block_on(adapter.request_device(&descriptor)).map_err(|error| format!("request device {error}"))?;
        MADE.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let scenes = Arc::new(glide::pipes(&device));
        let gpu = Self { instance, adapter, device, queue, composition, pipelines: Arc::new(Mutex::new(Vec::new())), scenes, small: Arc::new(Mutex::new(None)), name, kind };
        gpu.pipeline(wgpu::TextureFormat::Bgra8Unorm);
        Ok(gpu)
    }

    fn pipeline(&self, format: wgpu::TextureFormat) -> Pipeline {
        if let Some(found) = self.pipelines.lock().ok().and_then(|pipelines| pipelines.iter().find(|pipeline| pipeline.format == format).cloned()) {
            return found;
        }
        let device = &self.device;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pattern"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("pattern"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("pattern"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("pattern"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vertex"), compilation_options: wgpu::PipelineCompilationOptions::default(), buffers: &[] },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fragment"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let made = Pipeline { format, layout, pipeline };
        if let Ok(mut pipelines) = self.pipelines.lock() {
            pipelines.push(made.clone());
        }
        made
    }
}

fn surface_target(target: &Target) -> Result<wgpu::SurfaceTargetUnsafe, String> {
    match target {
        Target::Window { window, instance, .. } => {
            let mut handle = raw_window_handle::Win32WindowHandle::new(NonZeroIsize::new(*window).ok_or("the window handle is empty")?);
            handle.hinstance = NonZeroIsize::new(*instance);
            Ok(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: Some(raw_window_handle::RawDisplayHandle::Windows(raw_window_handle::WindowsDisplayHandle::new())),
                raw_window_handle: raw_window_handle::RawWindowHandle::Win32(handle),
            })
        }
        #[cfg(target_os = "macos")]
        Target::Layer(layer) => Ok(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(*layer)),
        #[cfg(not(target_os = "macos"))]
        Target::Layer(_) => Err("a Core Animation layer only exists on macOS".into()),
    }
}

impl Renderer {
    pub fn new(target: Target, pattern: &Pattern) -> Result<Self, String> {
        let gpu = Gpu::new(composition(&target))?;
        Self::on(&gpu, target, pattern)
    }

    pub fn on(gpu: &Gpu, target: Target, pattern: &Pattern) -> Result<Self, String> {
        if gpu.composition != composition(&target) {
            return Err("the device was made for another presentation system".into());
        }
        gpu.fits(pattern.width, pattern.height)?;
        let Gpu { instance, adapter, device, queue, name, .. } = gpu.clone();
        let surface = unsafe { instance.create_surface_unsafe(surface_target(&target)?) }.map_err(|error| format!("create surface {error}"))?;
        if !adapter.is_surface_supported(&surface) {
            return Err(format!("{name} cannot present to this surface"));
        }
        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities.formats.iter().copied().find(|format| !format.is_srgb()).or(capabilities.formats.first().copied()).ok_or("the surface offers no format")?;
        let mut config = surface.get_default_config(&adapter, pattern.width, pattern.height).ok_or("the surface has no default configuration")?;
        config.format = format;
        config.present_mode = wgpu::PresentMode::Fifo;
        if capabilities.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
            config.alpha_mode = wgpu::CompositeAlphaMode::Opaque;
        }
        surface.configure(&device, &config);
        let size = wgpu::Extent3d { width: pattern.width, height: pattern.height, depth_or_array_layers: 1 };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("picture"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &pattern.rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * pattern.width), rows_per_image: Some(pattern.height) },
            size,
        );
        let uniform = device.create_buffer(&wgpu::BufferDescriptor { label: Some("frame"), size: 16, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let Pipeline { layout, pipeline, .. } = gpu.pipeline(format);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pattern"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) }],
        });
        Ok(Self { device, queue, surface, pipeline, bind, uniform, width: pattern.width, height: pattern.height, adapter: name })
    }

    pub fn frame(&mut self, bar: Option<f32>) -> Result<(), String> {
        let values = [bar.unwrap_or(-1.0), self.width as f32, self.height as f32, BAR_HALF_WIDTH];
        let bytes: Vec<u8> = values.iter().flat_map(|value| value.to_le_bytes()).collect();
        self.queue.write_buffer(&self.uniform, 0, &bytes);
        let texture = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => texture,
            wgpu::CurrentSurfaceTexture::Timeout => return Err("the surface timed out".into()),
            wgpu::CurrentSurfaceTexture::Occluded => return Err("the surface is occluded".into()),
            wgpu::CurrentSurfaceTexture::Outdated => return Err("the surface is outdated".into()),
            wgpu::CurrentSurfaceTexture::Lost => return Err("the surface was lost".into()),
            wgpu::CurrentSurfaceTexture::Validation => return Err("the surface failed validation".into()),
        };
        let view = texture.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("frame"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit([encoder.finish()]);
        self.queue.present(texture);
        Ok(())
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        let _ = self.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: Some(std::time::Duration::from_secs(1)) });
    }
}

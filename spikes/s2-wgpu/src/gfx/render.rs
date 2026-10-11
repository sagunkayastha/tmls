//! The wgpu side: one pipeline that draws `grid::Instance` quads, the atlas texture upload, and
//! an offscreen target for rendering without a window.
//!
//! API for the front-end (per rendered frame):
//! 1. `renderer.prepare(&device, &queue, &mut atlas, &scene.layers(), (width, height))` — uploads
//!    the atlas rows that changed and all instances. Call it after `Scene::update*`.
//! 2. `renderer.draw(&mut encoder, &view, theme.bg)` — one render pass that clears and draws.
//!    The caller owns the encoder, so it can take its `queue.submit` timestamp itself.
//!
//! `Renderer::new(&device, format)` takes the surface format. Colours are sRGB bytes and are
//! blended as they are, like most terminals do: prefer a non-sRGB format (`Bgra8Unorm`,
//! `Rgba8Unorm`). With an `*Srgb` format the shader converts to linear and the colours stay
//! right, but glyph edges blend in linear light and look thinner.
//!
//! Headless: `Gpu::headless()` finds an adapter without a surface (hardware, else software),
//! `render_offscreen` draws into a texture of the renderer's format (use `OFFSCREEN_FORMAT`) and
//! returns its pixels.
//! `adapter_report()` lists what wgpu can see on this machine.

use anyhow::{Context, anyhow};

use super::SHADER_WGSL;
use super::atlas::Atlas;
use super::grid::Instance;

/// The format to build a `Renderer` with for `render_offscreen`: plain RGBA bytes.
pub const OFFSCREEN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

const ATLAS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const INSTANCE_SIZE: u64 = std::mem::size_of::<Instance>() as u64;
const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4, 3 => Unorm8x4, 4 => Uint32];

/// Mirrors `Globals` in `shader.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    viewport: [f32; 2],
    srgb_target: u32,
    _pad: u32,
}

/// A device and its queue, plus the instance and adapter they came from.
pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl Gpu {
    /// An adapter that needs no surface: the default choice, else a software ("fallback")
    /// adapter, else the first one any backend lists. `WGPU_BACKEND` and friends are honoured.
    pub fn headless() -> anyhow::Result<Gpu> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let request = |force_fallback_adapter| {
            let options = wgpu::RequestAdapterOptions {
                force_fallback_adapter,
                ..Default::default()
            };
            pollster::block_on(instance.request_adapter(&options)).ok()
        };
        let adapter = request(false)
            .or_else(|| request(true))
            .or_else(|| {
                pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()))
                    .into_iter()
                    .next()
            })
            .ok_or_else(|| {
                anyhow!("no wgpu adapter: no Vulkan driver and no GL/EGL context on this machine")
            })?;
        Gpu::from_adapter(instance, adapter)
    }

    /// Opens a device on an adapter the caller chose (the one compatible with its surface).
    pub fn from_adapter(instance: wgpu::Instance, adapter: wgpu::Adapter) -> anyhow::Result<Gpu> {
        let descriptor = wgpu::DeviceDescriptor {
            label: Some("s2-wgpu"),
            // Nothing beyond what a GLES 3 class device has, at whatever sizes this one offers.
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            ..Default::default()
        };
        let (device, queue) = pollster::block_on(adapter.request_device(&descriptor))
            .with_context(|| format!("request_device on {:?}", adapter.get_info().name))?;
        Ok(Gpu {
            instance,
            adapter,
            device,
            queue,
        })
    }
}

/// One line per adapter wgpu can enumerate here (any backend), for logs and reports.
pub fn adapter_report() -> Vec<String> {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()))
        .iter()
        .map(|adapter| {
            let info = adapter.get_info();
            format!(
                "{:?} / {:?} / {} / driver {} {}",
                info.backend, info.device_type, info.name, info.driver, info.driver_info
            )
        })
        .collect()
}

pub struct Renderer {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    globals: wgpu::Buffer,
    format: wgpu::TextureFormat,
    instances: wgpu::Buffer,
    /// Instances the buffer has room for.
    capacity: u64,
    /// Instances to draw.
    count: u32,
    /// Atlas texture with its bind group; `None` until the first `prepare`.
    atlas: Option<(wgpu::Texture, wgpu::BindGroup)>,
    /// Reused between `prepare` calls.
    scratch: Vec<Instance>,
}

impl Renderer {
    /// `format`: the format of the views `draw` will be given.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Renderer {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("s2-wgpu quads"),
            source: wgpu::ShaderSource::Wgsl(SHADER_WGSL.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("s2-wgpu globals + atlas"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("s2-wgpu quads"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("s2-wgpu quads"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: INSTANCE_SIZE,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &INSTANCE_ATTRIBUTES,
                })],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("s2-wgpu globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let capacity = 4096;
        Renderer {
            pipeline,
            layout,
            globals,
            format,
            instances: instance_buffer(device, capacity),
            capacity,
            count: 0,
            atlas: None,
            scratch: Vec::new(),
        }
    }

    /// Uploads what the next `draw` needs: the target size, the changed part of the atlas, and
    /// the instances of all `layers`, which are drawn in the order given.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        atlas: &mut Atlas,
        layers: &[&[Instance]],
        size: (u32, u32),
    ) {
        let globals = Globals {
            viewport: [size.0.max(1) as f32, size.1.max(1) as f32],
            srgb_target: self.format.is_srgb() as u32,
            _pad: 0,
        };
        queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));

        self.upload_atlas(device, queue, atlas);

        // One buffer, one draw call from instance 0: no base-instance support needed (GLES).
        self.scratch.clear();
        for layer in layers {
            self.scratch.extend_from_slice(layer);
        }
        let count = self.scratch.len() as u64;
        if count > self.capacity {
            self.capacity = count.next_power_of_two();
            self.instances = instance_buffer(device, self.capacity);
        }
        if count > 0 {
            queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(&self.scratch));
        }
        self.count = count as u32;
    }

    fn upload_atlas(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, atlas: &mut Atlas) {
        let dirty = atlas.take_dirty();
        let image = atlas.image();
        let extent = wgpu::Extent3d {
            width: image.width,
            height: image.height,
            depth_or_array_layers: 1,
        };
        let current = self
            .atlas
            .as_ref()
            .filter(|(texture, _)| texture.size() == extent);
        // A new or resized texture is filled whole; otherwise only the rows that changed.
        let rows = match (current, dirty) {
            (Some(_), None) => return,
            (Some(_), Some(rows)) => rows,
            (None, _) => {
                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("s2-wgpu atlas"),
                    size: extent,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: ATLAS_FORMAT,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("s2-wgpu globals + atlas"),
                    layout: &self.layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: self.globals.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(&view),
                        },
                    ],
                });
                self.atlas = Some((texture, bind_group));
                (0, image.height)
            }
        };
        let (y0, y1) = (rows.0.min(image.height), rows.1.min(image.height));
        if y0 >= y1 {
            return;
        }
        let Some((texture, _)) = &self.atlas else {
            return;
        };
        let row_bytes = image.width as usize * 4;
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: 0, y: y0, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            &image.rgba[y0 as usize * row_bytes..y1 as usize * row_bytes],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row_bytes as u32),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: image.width,
                height: y1 - y0,
                depth_or_array_layers: 1,
            },
        );
    }

    /// One render pass into `view`: clear to `clear`, then every prepared instance.
    pub fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        clear: [u8; 3],
    ) {
        // A clear value is in the target's own space: linear for an sRGB target.
        let channel = |c: u8| {
            let c = c as f64 / 255.0;
            if !self.format.is_srgb() {
                c
            } else if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("s2-wgpu grid"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: channel(clear[0]),
                        g: channel(clear[1]),
                        b: channel(clear[2]),
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let Some((_, bind_group)) = &self.atlas else {
            return;
        };
        if self.count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.set_vertex_buffer(0, self.instances.slice(..self.count as u64 * INSTANCE_SIZE));
        pass.draw(0..4, 0..self.count);
    }
}

fn instance_buffer(device: &wgpu::Device, capacity: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("s2-wgpu instances"),
        size: capacity * INSTANCE_SIZE,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// Draws what `renderer` has prepared into a `width` x `height` texture of the renderer's format
/// and reads it back: `width * height * 4` bytes, top row first — RGBA for `OFFSCREEN_FORMAT`
/// (any four-byte format works; the bytes come back in that format's channel order and
/// encoding). The renderer must have been prepared with the same size.
pub fn render_offscreen(
    gpu: &Gpu,
    renderer: &Renderer,
    width: u32,
    height: u32,
    clear: [u8; 3],
) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(width > 0 && height > 0, "empty offscreen target");
    let extent = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("s2-wgpu offscreen"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: renderer.format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    // Rows in a texture-to-buffer copy are padded to a multiple of 256 bytes.
    let row_bytes = width * 4;
    let padded =
        row_bytes.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("s2-wgpu readback"),
        size: padded as u64 * height as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("s2-wgpu offscreen"),
        });
    renderer.draw(&mut encoder, &view, clear);
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: None,
            },
        },
        extent,
    );
    gpu.queue.submit([encoder.finish()]);

    let (tx, rx) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .context("waiting for the offscreen frame")?;
    rx.recv()
        .context("the map callback never ran")?
        .context("mapping the readback buffer")?;
    let mapped = readback
        .slice(..)
        .get_mapped_range()
        .context("reading the readback buffer")?;
    let mut pixels = Vec::with_capacity((row_bytes * height) as usize);
    for row in mapped.chunks_exact(padded as usize) {
        pixels.extend_from_slice(&row[..row_bytes as usize]);
    }
    drop(mapped);
    readback.unmap();
    Ok(pixels)
}

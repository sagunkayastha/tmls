//! The picture in the file panel on the GPU: its texture and a pipeline of its own that draws
//! one quad with a part of it.
//!
//! Apart from the grid's renderer (`s2_wgpu::gfx::render`), which draws glyphs from an atlas
//! texel for texel: a picture is one large texture of its own, sampled with a linear filter
//! because it is drawn smaller than it is when it is fitted. Nothing here runs while the panel
//! is closed or shows text — the app calls `draw` only for a panel with a picture — so the
//! path from a key to its frame is the grid's alone.
//!
//! 1. `PictureRenderer::new(&device, format)` once: the pipeline, for views of `format`.
//! 2. `set(&device, &queue, Some(&picture))` when a picture opens: the texture is made and
//!    filled once; the caller can drop the pixels. `set(.., None)` frees it.
//! 3. `draw(&mut encoder, &view, viewport, dest, source, clip)` each frame the picture is in
//!    view, after the grid's pass: a second pass that keeps what is there (`LoadOp::Load`)
//!    and draws the texels of `source` on the pixels of `dest` (`viewer::place` gives both),
//!    nothing outside `clip`.
//!
//! Colours are as the grid's: sRGB bytes into the target as they are (`picture.wgsl`).

use crate::draw::Rect;
use crate::viewer::Picture;

/// sRGB bytes with alpha that is not multiplied in, as `viewer::decode` gives them.
const TEXTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
/// Bytes of `Params` in `picture.wgsl`.
const PARAMS_SIZE: usize = 64;

/// A picture on the GPU.
struct Loaded {
    /// Kept for as long as the bind group: dropping both frees the memory.
    _texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    size: (u32, u32),
    /// For the parameters of each `draw`.
    queue: wgpu::Queue,
}

pub struct PictureRenderer {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    params: wgpu::Buffer,
    format: wgpu::TextureFormat,
    loaded: Option<Loaded>,
}

impl PictureRenderer {
    /// `format`: the format of the views `draw` will be given.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> PictureRenderer {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("tmls-rs picture"),
            source: wgpu::ShaderSource::Wgsl(include_str!("picture.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("tmls-rs picture"),
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
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("tmls-rs picture"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("tmls-rs picture"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
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
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("tmls-rs picture"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tmls-rs picture params"),
            size: PARAMS_SIZE as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        PictureRenderer {
            pipeline,
            layout,
            sampler,
            params,
            format,
            loaded: None,
        }
    }

    /// The picture to draw from now on, uploaded once, or none: the texture of the one
    /// before is freed either way. Returns whether there is one to draw now: not for `None`,
    /// and not for a picture this device has no texture for (one of no size, of a size its
    /// pixels do not have, or longer on a side than the device's largest texture).
    pub fn set(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        picture: Option<&Picture>,
    ) -> bool {
        self.loaded = None;
        let Some(picture) = picture else {
            return false;
        };
        let (width, height) = (picture.width, picture.height);
        let most = device.limits().max_texture_dimension_2d;
        let bytes = width as u64 * height as u64 * 4;
        if width == 0 || height == 0 || width > most || height > most {
            return false;
        }
        if picture.rgba.len() as u64 != bytes {
            return false;
        }
        let extent = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("tmls-rs picture"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TEXTURE_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &picture.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: None,
            },
            extent,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tmls-rs picture"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        self.loaded = Some(Loaded {
            _texture: texture,
            bind_group,
            size: (width, height),
            queue: queue.clone(),
        });
        true
    }

    /// The size of the picture that is loaded, in texels.
    pub fn size(&self) -> Option<(u32, u32)> {
        self.loaded.as_ref().map(|loaded| loaded.size)
    }

    /// One render pass into `view`, which is `viewport` pixels: what is there stays, and the
    /// texels of `source` are drawn on the pixels of `dest`, those inside `clip` only. No pass
    /// at all without a picture or with nothing of it to see. One call for each submit: the
    /// parameters are the last call's.
    pub fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        viewport: (u32, u32),
        dest: Rect,
        source: Rect,
        clip: Rect,
    ) {
        let Some(loaded) = &self.loaded else {
            return;
        };
        let Some((x, y, width, height)) = scissor(clip, viewport) else {
            return;
        };
        // Not `<= 0.0`: a size that is no number draws nothing either.
        if !(dest.w > 0.0 && dest.h > 0.0 && source.w > 0.0 && source.h > 0.0) {
            return;
        }
        let floats = [
            [dest.x, dest.y, dest.w, dest.h],
            [source.x, source.y, source.w, source.h],
            [
                viewport.0 as f32,
                viewport.1 as f32,
                loaded.size.0 as f32,
                loaded.size.1 as f32,
            ],
        ];
        let mut params = [0u8; PARAMS_SIZE];
        for (bytes, value) in params.chunks_exact_mut(4).zip(floats.iter().flatten()) {
            bytes.copy_from_slice(&value.to_ne_bytes());
        }
        params[48..52].copy_from_slice(&(self.format.is_srgb() as u32).to_ne_bytes());
        loaded.queue.write_buffer(&self.params, 0, &params);

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("tmls-rs picture"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_scissor_rect(x, y, width, height);
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &loaded.bind_group, &[]);
        pass.draw(0..4, 0..1);
    }
}

/// The whole pixels of `clip` that are inside a target of `viewport` pixels, as a scissor
/// rectangle (x, y, width, height); `None` if there is none.
pub fn scissor(clip: Rect, viewport: (u32, u32)) -> Option<(u32, u32, u32, u32)> {
    let edge = |from: f32, size: f32, limit: u32| {
        let start = from.max(0.0).ceil().min(limit as f32);
        let end = (from + size).floor().clamp(0.0, limit as f32);
        // Also for a rectangle that is no number.
        (end > start).then_some((start as u32, (end - start) as u32))
    };
    let (x, width) = edge(clip.x, clip.w, viewport.0)?;
    let (y, height) = edge(clip.y, clip.h, viewport.1)?;
    Some((x, y, width, height))
}

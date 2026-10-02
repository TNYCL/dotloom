//! GPU side: pipelines, buffers and the frame loop.

use bytemuck::{Pod, Zeroable};
use dotloom_scene::{Overlay, SceneDelta, flags};
use serde::Serialize;

use crate::RenderError;
use crate::cache::{FrameParams, PrepareStats, SceneCache};
use crate::color::{Theme, unpack};
use crate::overlay::{Grid, grid_mesh, screen_overlay, world_overlay};
use crate::tess::{FillVertex, GlyphInstance, LineInstance, Look, Mesh, tessellate_item};
use crate::text::{ATLAS_SIZE, TextSystem};
use crate::view::{View, lod_tolerance};

/// Uniform slot stride (WebGPU/WebGL2 minimum dynamic offset alignment).
const UNIFORM_STRIDE: u64 = 256;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Xf {
    m: [f32; 4],
    tv: [f32; 4],
    p: [f32; 4],
}

/// A growable GPU buffer.
#[derive(Debug, Default)]
pub(crate) struct GpuVec {
    buf: Option<wgpu::Buffer>,
    cap: u64,
    len: u64,
}

impl GpuVec {
    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, data: &[u8], usage: wgpu::BufferUsages) {
        self.len = data.len() as u64;
        if data.is_empty() {
            return;
        }
        if self.buf.is_none() || self.cap < self.len {
            if let Some(b) = self.buf.take() {
                b.destroy();
            }
            let cap = self.len.next_power_of_two().max(256);
            self.buf = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("dotloom chunk"),
                size: cap,
                usage: usage | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.cap = cap;
        }
        if let Some(b) = &self.buf {
            queue.write_buffer(b, 0, data);
        }
    }

    fn slice(&self) -> Option<wgpu::BufferSlice<'_>> {
        if self.len == 0 { None } else { self.buf.as_ref().map(|b| b.slice(..self.len)) }
    }

    /// Elements `range` of a buffer of `stride`-byte elements. Instanced draws bind
    /// a sub-slice instead of using a non-zero first instance, which WebGL2 lacks.
    fn elements(&self, range: &core::ops::Range<u32>, stride: u64) -> Option<wgpu::BufferSlice<'_>> {
        let (start, end) = (u64::from(range.start) * stride, u64::from(range.end) * stride);
        if start >= end || end > self.len {
            return None;
        }
        self.buf.as_ref().map(|b| b.slice(start..end))
    }

    fn destroy(self) {
        if let Some(b) = self.buf {
            b.destroy();
        }
    }

    fn bytes(&self) -> u64 {
        self.cap
    }
}

/// GPU buffers of one chunk (or transient mesh).
#[derive(Debug, Default)]
pub struct ChunkBuffers {
    lines: GpuVec,
    fill_vertices: GpuVec,
    fill_indices: GpuVec,
    glyphs: GpuVec,
}

impl ChunkBuffers {
    pub(crate) fn is_empty(&self) -> bool {
        self.lines.buf.is_none()
            && self.fill_vertices.buf.is_none()
            && self.fill_indices.buf.is_none()
            && self.glyphs.buf.is_none()
    }

    pub(crate) fn destroy(self) {
        self.lines.destroy();
        self.fill_vertices.destroy();
        self.fill_indices.destroy();
        self.glyphs.destroy();
    }

    fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        lines: &[LineInstance],
        fv: &[FillVertex],
        fi: &[u32],
        glyphs: &[GlyphInstance],
    ) {
        self.lines.upload(device, queue, bytemuck::cast_slice(lines), wgpu::BufferUsages::VERTEX);
        self.fill_vertices.upload(device, queue, bytemuck::cast_slice(fv), wgpu::BufferUsages::VERTEX);
        self.fill_indices.upload(device, queue, bytemuck::cast_slice(fi), wgpu::BufferUsages::INDEX);
        self.glyphs.upload(device, queue, bytemuck::cast_slice(glyphs), wgpu::BufferUsages::VERTEX);
    }

    fn bytes(&self) -> u64 {
        self.lines.bytes() + self.fill_vertices.bytes() + self.fill_indices.bytes() + self.glyphs.bytes()
    }
}

/// Renderer options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RendererOptions {
    /// MSAA sample count (1 or 4); 4 anti-aliases fills.
    pub sample_count: u32,
}

impl Default for RendererOptions {
    fn default() -> Self {
        Self { sample_count: 4 }
    }
}

/// Statistics of the last frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameStats {
    /// Scene items.
    pub items: u32,
    /// Chunks in the scene.
    pub chunks: u32,
    /// Chunks drawn (after culling).
    pub chunks_drawn: u32,
    /// Chunks whose geometry was rebuilt this frame.
    pub chunks_rebuilt: u32,
    /// Items tessellated this frame.
    pub items_tessellated: u32,
    /// Draw calls.
    pub draw_calls: u32,
    /// Line segments drawn.
    pub line_segments: u32,
    /// Fill triangles drawn.
    pub triangles: u32,
    /// Glyph quads drawn.
    pub glyphs: u32,
    /// Bytes allocated in GPU buffers for chunks.
    pub gpu_bytes: u64,
}

/// One draw batch of the frame.
struct Batch {
    slot: u64,
    chunk: Option<usize>,
    transient: Option<usize>,
}

/// The wgpu renderer. Consumes scene deltas; owns only derived GPU/CPU caches.
pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    sample_count: u32,
    line_pipeline: wgpu::RenderPipeline,
    fill_pipeline: wgpu::RenderPipeline,
    glyph_pipeline: wgpu::RenderPipeline,
    uniform_layout: wgpu::BindGroupLayout,
    uniform_buf: wgpu::Buffer,
    uniform_slots: u64,
    uniform_bind: wgpu::BindGroup,
    atlas: wgpu::Texture,
    atlas_bind: wgpu::BindGroup,
    msaa: Option<(wgpu::Texture, wgpu::TextureView)>,
    /// Grid, hover, world overlay, screen overlay.
    transient: [ChunkBuffers; 4],
    transient_mesh: [Mesh; 4],
    scene: SceneCache,
    text: TextSystem,
    view: View,
    theme: Theme,
    grid: Grid,
    overlay: Overlay,
    hover: Option<u64>,
    last: FrameStats,
    disposed: bool,
}

impl std::fmt::Debug for Renderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Renderer")
            .field("format", &self.format)
            .field("sample_count", &self.sample_count)
            .field("scene", &self.scene)
            .finish_non_exhaustive()
    }
}

fn uniform_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("dotloom xf"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: true,
                min_binding_size: wgpu::BufferSize::new(core::mem::size_of::<Xf>() as u64),
            },
            count: None,
        }],
    })
}

fn uniform_bind(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, buf: &wgpu::Buffer) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("dotloom xf"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: buf,
                offset: 0,
                size: wgpu::BufferSize::new(core::mem::size_of::<Xf>() as u64),
            }),
        }],
    })
}

fn uniform_buffer(device: &wgpu::Device, slots: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("dotloom xf"),
        size: slots * UNIFORM_STRIDE,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

const LINE_ATTRS: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
    0 => Float32x2, 1 => Float32x2, 2 => Unorm8x4, 3 => Float32, 4 => Float32, 5 => Float32x4
];
const FILL_ATTRS: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![0 => Float32x2, 1 => Unorm8x4];
const GLYPH_ATTRS: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
    0 => Float32x2, 1 => Float32x2, 2 => Float32x2, 3 => Float32x4, 4 => Unorm8x4, 5 => Float32
];

impl Renderer {
    /// Create a renderer drawing into targets of `format`.
    ///
    /// The device should be created with limits no larger than
    /// [`wgpu::Limits::downlevel_webgl2_defaults`] needs; the renderer uses only
    /// WebGL2-compatible features on every backend.
    ///
    /// # Errors
    /// [`RenderError::Font`] if the bundled font fails to load (never expected).
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
        options: RendererOptions,
    ) -> Result<Self, RenderError> {
        let sample_count = if options.sample_count >= 4 { 4 } else { 1 };
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("dotloom shaders"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders.wgsl").into()),
        });
        let uniform_layout = uniform_layout(&device);
        let atlas_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("dotloom atlas"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let plain = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("dotloom plain"),
            bind_group_layouts: &[Some(&uniform_layout)],
            immediate_size: 0,
        });
        let textured = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("dotloom textured"),
            bind_group_layouts: &[Some(&uniform_layout), Some(&atlas_layout)],
            immediate_size: 0,
        });
        let target = [Some(wgpu::ColorTargetState {
            format,
            blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            write_mask: wgpu::ColorWrites::ALL,
        })];
        let make = |label: &str,
                    layout: &wgpu::PipelineLayout,
                    vs: &str,
                    fs: &str,
                    buffer: wgpu::VertexBufferLayout<'_>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vs),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[Some(buffer)],
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    ..wgpu::PrimitiveState::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState { count: sample_count, mask: !0, alpha_to_coverage_enabled: false },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &target,
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let line_pipeline = make(
            "dotloom lines",
            &plain,
            "vs_line",
            "fs_line",
            wgpu::VertexBufferLayout {
                array_stride: core::mem::size_of::<LineInstance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &LINE_ATTRS,
            },
        );
        let fill_pipeline = make(
            "dotloom fills",
            &plain,
            "vs_fill",
            "fs_fill",
            wgpu::VertexBufferLayout {
                array_stride: core::mem::size_of::<FillVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &FILL_ATTRS,
            },
        );
        let glyph_pipeline = make(
            "dotloom glyphs",
            &textured,
            "vs_glyph",
            "fs_glyph",
            wgpu::VertexBufferLayout {
                array_stride: core::mem::size_of::<GlyphInstance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &GLYPH_ATTRS,
            },
        );
        let atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("dotloom glyph atlas"),
            size: wgpu::Extent3d { width: ATLAS_SIZE as u32, height: ATLAS_SIZE as u32, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("dotloom atlas"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..wgpu::SamplerDescriptor::default()
        });
        let atlas_view = atlas.create_view(&wgpu::TextureViewDescriptor::default());
        let atlas_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("dotloom atlas"),
            layout: &atlas_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&atlas_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
            ],
        });
        let uniform_slots = 64;
        let uniform_buf = uniform_buffer(&device, uniform_slots);
        let uniform_bind = uniform_bind(&device, &uniform_layout, &uniform_buf);
        Ok(Self {
            device,
            queue,
            format,
            sample_count,
            line_pipeline,
            fill_pipeline,
            glyph_pipeline,
            uniform_layout,
            uniform_buf,
            uniform_slots,
            uniform_bind,
            atlas,
            atlas_bind,
            msaa: None,
            transient: Default::default(),
            transient_mesh: Default::default(),
            scene: SceneCache::default(),
            text: TextSystem::with_default_font()?,
            view: View::default(),
            theme: Theme::default(),
            grid: Grid::default(),
            overlay: Overlay::default(),
            hover: None,
            last: FrameStats::default(),
            disposed: false,
        })
    }

    /// The device.
    #[must_use]
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// The queue.
    #[must_use]
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// Target format.
    #[must_use]
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// MSAA sample count in use.
    #[must_use]
    pub fn sample_count(&self) -> u32 {
        self.sample_count
    }

    /// Apply a binary scene delta (`dotloom-scene` format).
    ///
    /// # Errors
    /// [`RenderError::Scene`] for malformed input; the scene is left unchanged.
    pub fn apply_delta_bytes(&mut self, bytes: &[u8]) -> Result<(), RenderError> {
        let d = SceneDelta::decode(bytes).map_err(|e| RenderError::Scene(e.to_string()))?;
        self.apply_delta(d);
        Ok(())
    }

    /// Apply a decoded scene delta.
    pub fn apply_delta(&mut self, delta: SceneDelta) {
        self.scene.apply(delta);
    }

    /// The scene cache.
    #[must_use]
    pub fn scene(&self) -> &SceneCache {
        &self.scene
    }

    /// Set the camera.
    ///
    /// # Errors
    /// [`RenderError::InvalidView`] for non-finite or out-of-range values.
    pub fn set_view(&mut self, view: View) -> Result<(), RenderError> {
        view.validate()?;
        self.view = view;
        Ok(())
    }

    /// Current camera.
    #[must_use]
    pub fn view(&self) -> View {
        self.view
    }

    /// Set colors (rebuilds cached geometry).
    pub fn set_theme(&mut self, theme: Theme) {
        if theme != self.theme {
            self.theme = theme;
            self.scene.invalidate_all();
        }
    }

    /// Current theme.
    #[must_use]
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Set grid settings.
    pub fn set_grid(&mut self, grid: Grid) {
        self.grid = grid;
    }

    /// Set the interaction overlay.
    pub fn set_overlay(&mut self, overlay: Overlay) {
        self.overlay = overlay;
    }

    /// Highlight one item as hovered (drawn on top as feedback).
    pub fn set_hover(&mut self, id: Option<u64>) {
        self.hover = id;
    }

    /// Statistics of the last frame.
    #[must_use]
    pub fn last_stats(&self) -> FrameStats {
        self.last
    }

    fn xf_world(&self, origin: [f64; 2], size: (u32, u32), alpha: f32) -> Xf {
        let s = self.view.device_scale();
        let (w, h) = (f64::from(size.0), f64::from(size.1));
        let e = (origin[0] - self.view.center[0]) * s + w * 0.5;
        let f = h * 0.5 - (origin[1] - self.view.center[1]) * s;
        Xf {
            m: [s as f32, 0.0, 0.0, -s as f32],
            tv: [e as f32, f as f32, size.0 as f32, size.1 as f32],
            p: [self.view.dpr as f32, s as f32, alpha, 0.0],
        }
    }

    fn xf_screen(&self, size: (u32, u32)) -> Xf {
        Xf {
            m: [1.0, 0.0, 0.0, 1.0],
            tv: [0.0, 0.0, size.0 as f32, size.1 as f32],
            p: [self.view.dpr as f32, 1.0, 1.0, 0.0],
        }
    }

    fn upload_atlas(&mut self) {
        let Some(d) = self.text.dirty.take() else { return };
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.atlas,
                mip_level: 0,
                origin: wgpu::Origin3d { x: d.x as u32, y: d.y as u32, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            &self.text.atlas,
            wgpu::TexelCopyBufferLayout {
                offset: (d.y * ATLAS_SIZE + d.x) as u64,
                bytes_per_row: Some(ATLAS_SIZE as u32),
                rows_per_image: Some(d.h as u32),
            },
            wgpu::Extent3d { width: d.w as u32, height: d.h as u32, depth_or_array_layers: 1 },
        );
    }

    fn ensure_msaa(&mut self, size: (u32, u32)) {
        if self.sample_count == 1 {
            return;
        }
        let ok = self.msaa.as_ref().is_some_and(|(t, _)| t.width() == size.0 && t.height() == size.1);
        if ok {
            return;
        }
        if let Some((t, _)) = self.msaa.take() {
            t.destroy();
        }
        let t = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("dotloom msaa"),
            size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: self.sample_count,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let v = t.create_view(&wgpu::TextureViewDescriptor::default());
        self.msaa = Some((t, v));
    }

    /// Draw one frame into `target` (its size defines the viewport in device pixels).
    ///
    /// # Errors
    /// [`RenderError::Disposed`] after [`Renderer::dispose`].
    pub fn render(&mut self, target: &wgpu::Texture) -> Result<FrameStats, RenderError> {
        if self.disposed {
            return Err(RenderError::Disposed);
        }
        let size = (target.width().max(1), target.height().max(1));
        let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let lod = self.view.lod();
        let tol = lod_tolerance(lod);
        let mut ps = PrepareStats::default();
        // Cull with the target size actually drawn.
        let s = self.view.device_scale();
        let half = (f64::from(size.0) * 0.5 / s, f64::from(size.1) * 0.5 / s);
        let visible = dotloom_geometry::Aabb::from_corners(
            dotloom_geometry::Point::new(self.view.center[0] - half.0, self.view.center[1] - half.1),
            dotloom_geometry::Point::new(self.view.center[0] + half.0, self.view.center[1] + half.1),
        );
        // Strokes and markers extend a few pixels beyond item boxes.
        let margin = 16.0 / self.view.scale;
        let fp = FrameParams { visible, margin, lod, lod_tol: tol, theme: &self.theme };
        let vis = self.scene.prepare(&fp, &mut self.text, &mut ps);

        // Transient meshes: grid, hover, world overlay, screen overlay.
        let hover_mesh = self
            .hover
            .and_then(|id| self.scene.item(id).cloned())
            .and_then(|it| {
                tessellate_item(&it, Look { flags: it.flags | flags::HOVER }, tol, &self.theme, &mut self.text)
            })
            .unwrap_or_default();
        self.transient_mesh = [
            grid_mesh(&self.grid, &self.view, size, &self.theme),
            hover_mesh,
            world_overlay(&self.overlay, &self.view, tol, &self.theme),
            screen_overlay(&self.overlay, &self.view, size, &self.theme),
        ];
        self.upload_atlas();

        // Upload changed chunks.
        for &ci in &vis {
            let c = &mut self.scene.chunks[ci];
            if c.uploaded != c.built
                && let Some(d) = &c.data
            {
                c.gpu.upload(&self.device, &self.queue, &d.lines, &d.fill_vertices, &d.fill_indices, &d.glyphs);
                c.uploaded = c.built;
            }
        }
        for (b, m) in self.transient.iter_mut().zip(self.transient_mesh.iter()) {
            b.upload(&self.device, &self.queue, &m.lines, &m.fill_vertices, &m.fill_indices, &m.glyphs);
        }

        // Uniforms: [grid, chunks..., hover, world overlay, screen overlay].
        let mut xfs: Vec<Xf> = Vec::with_capacity(vis.len() + 4);
        let mut batches: Vec<Batch> = Vec::with_capacity(vis.len() + 4);
        xfs.push(self.xf_screen(size));
        batches.push(Batch { slot: 0, chunk: None, transient: Some(0) });
        for &ci in &vis {
            let origin = self.scene.chunks[ci].data.as_ref().map_or([0.0, 0.0], |d| d.origin);
            batches.push(Batch { slot: xfs.len() as u64, chunk: Some(ci), transient: None });
            xfs.push(self.xf_world(origin, size, 1.0));
        }
        let hover_origin = self.transient_mesh[1].origin;
        batches.push(Batch { slot: xfs.len() as u64, chunk: None, transient: Some(1) });
        xfs.push(self.xf_world(hover_origin, size, 1.0));
        let world_origin = self.transient_mesh[2].origin;
        batches.push(Batch { slot: xfs.len() as u64, chunk: None, transient: Some(2) });
        xfs.push(self.xf_world(world_origin, size, 1.0));
        batches.push(Batch { slot: xfs.len() as u64, chunk: None, transient: Some(3) });
        xfs.push(self.xf_screen(size));
        let needed = xfs.len() as u64;
        if needed > self.uniform_slots {
            self.uniform_slots = needed.next_power_of_two();
            self.uniform_buf.destroy();
            self.uniform_buf = uniform_buffer(&self.device, self.uniform_slots);
            self.uniform_bind = uniform_bind(&self.device, &self.uniform_layout, &self.uniform_buf);
        }
        let mut bytes = vec![0u8; (needed * UNIFORM_STRIDE) as usize];
        for (i, x) in xfs.iter().enumerate() {
            let at = i * UNIFORM_STRIDE as usize;
            bytes[at..at + core::mem::size_of::<Xf>()].copy_from_slice(bytemuck::bytes_of(x));
        }
        self.queue.write_buffer(&self.uniform_buf, 0, &bytes);

        self.ensure_msaa(size);
        let bg = unpack(self.theme.background);
        let clear = wgpu::Color {
            r: f64::from(bg[0] * bg[3]),
            g: f64::from(bg[1] * bg[3]),
            b: f64::from(bg[2] * bg[3]),
            a: f64::from(bg[3]),
        };
        let mut stats = FrameStats {
            items: self.scene.len() as u32,
            chunks: self.scene.chunks.len() as u32,
            chunks_drawn: vis.len() as u32,
            chunks_rebuilt: ps.rebuilt_chunks,
            items_tessellated: ps.tessellated_items,
            ..FrameStats::default()
        };
        let mut encoder =
            self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("dotloom frame") });
        {
            let (view, resolve) = match &self.msaa {
                Some((_, v)) => (v, Some(&target_view)),
                None => (&target_view, None),
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("dotloom scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: resolve,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        store: if resolve.is_some() { wgpu::StoreOp::Discard } else { wgpu::StoreOp::Store },
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(1, &self.atlas_bind, &[]);
            for b in &batches {
                let offset = (b.slot * UNIFORM_STRIDE) as u32;
                pass.set_bind_group(0, &self.uniform_bind, &[offset]);
                let (buffers, runs): (&ChunkBuffers, Vec<crate::cache::Run>) = match (b.chunk, b.transient) {
                    (Some(ci), _) => {
                        let c = &self.scene.chunks[ci];
                        (&c.gpu, c.data.as_ref().map(|d| d.runs.clone()).unwrap_or_default())
                    }
                    (None, Some(t)) => {
                        let m = &self.transient_mesh[t];
                        let run = crate::cache::Run {
                            fills: 0..m.fill_indices.len() as u32,
                            lines: 0..m.lines.len() as u32,
                            glyphs: 0..m.glyphs.len() as u32,
                        };
                        (&self.transient[t], vec![run])
                    }
                    (None, None) => continue,
                };
                for r in runs {
                    if !r.fills.is_empty()
                        && let (Some(v), Some(i)) = (buffers.fill_vertices.slice(), buffers.fill_indices.slice())
                    {
                        pass.set_pipeline(&self.fill_pipeline);
                        pass.set_vertex_buffer(0, v);
                        pass.set_index_buffer(i, wgpu::IndexFormat::Uint32);
                        pass.draw_indexed(r.fills.clone(), 0, 0..1);
                        stats.draw_calls += 1;
                        stats.triangles += r.fills.len() as u32 / 3;
                    }
                    if let Some(v) = buffers.lines.elements(&r.lines, core::mem::size_of::<LineInstance>() as u64) {
                        pass.set_pipeline(&self.line_pipeline);
                        pass.set_vertex_buffer(0, v);
                        pass.draw(0..6, 0..r.lines.len() as u32);
                        stats.draw_calls += 1;
                        stats.line_segments += r.lines.len() as u32;
                    }
                    if let Some(v) = buffers.glyphs.elements(&r.glyphs, core::mem::size_of::<GlyphInstance>() as u64) {
                        pass.set_pipeline(&self.glyph_pipeline);
                        pass.set_vertex_buffer(0, v);
                        pass.draw(0..6, 0..r.glyphs.len() as u32);
                        stats.draw_calls += 1;
                        stats.glyphs += r.glyphs.len() as u32;
                    }
                }
            }
        }
        self.queue.submit([encoder.finish()]);
        stats.gpu_bytes = self.scene.chunks.iter().map(|c| c.gpu.bytes()).sum();
        self.last = stats;
        Ok(stats)
    }

    /// Release all GPU resources now (buffers, textures). The renderer cannot draw
    /// afterwards. Dropping the renderer also releases them.
    pub fn dispose(&mut self) {
        if self.disposed {
            return;
        }
        self.disposed = true;
        self.scene.destroy_gpu();
        for b in core::mem::take(&mut self.transient) {
            b.destroy();
        }
        if let Some((t, _)) = self.msaa.take() {
            t.destroy();
        }
        self.atlas.destroy();
        self.uniform_buf.destroy();
    }

    /// Whether [`Renderer::dispose`] was called.
    #[must_use]
    pub fn is_disposed(&self) -> bool {
        self.disposed
    }
}

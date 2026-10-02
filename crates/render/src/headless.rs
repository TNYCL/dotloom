//! Offscreen rendering on native backends (feature `png`).
//!
//! Used by the CLI's `export --format png` and by visual regression tests. Needs
//! a GPU or a software adapter (e.g. Mesa lavapipe/llvmpipe, WARP); when none is
//! available the constructor returns [`RenderError::NoAdapter`] instead of
//! silently producing a different rasterization.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::gpu::{Renderer, RendererOptions};
use crate::{FrameStats, RenderError};

/// Information about the adapter used.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AdapterSummary {
    /// Adapter name.
    pub name: String,
    /// Backend (`vulkan`, `dx12`, `metal`, `gl`).
    pub backend: String,
    /// Device type (`DiscreteGpu`, `Cpu`, …).
    pub device_type: String,
    /// Driver description.
    pub driver: String,
}

/// An offscreen renderer.
#[derive(Debug)]
pub struct Headless {
    renderer: Renderer,
    adapter: AdapterSummary,
}

/// An RGBA8 image (straight alpha, sRGB-encoded values as drawn).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Row-major RGBA8 pixels.
    pub pixels: Vec<u8>,
}

impl Image {
    /// Encode as PNG.
    ///
    /// # Errors
    /// [`RenderError::Readback`] if encoding fails.
    pub fn to_png(&self) -> Result<Vec<u8>, RenderError> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, self.width, self.height);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().map_err(|e| RenderError::Readback(e.to_string()))?;
            w.write_image_data(&self.pixels).map_err(|e| RenderError::Readback(e.to_string()))?;
        }
        Ok(out)
    }
}

impl Headless {
    /// Create an offscreen renderer on the first available native adapter
    /// (`WGPU_BACKEND` selects backends, e.g. `vulkan`, `dx12`, `gl`).
    ///
    /// # Errors
    /// [`RenderError::NoAdapter`] or [`RenderError::Device`] when no usable GPU or
    /// software adapter exists.
    pub fn new(options: RendererOptions) -> Result<Self, RenderError> {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::from_env().unwrap_or(wgpu::Backends::PRIMARY | wgpu::Backends::GL);
        let instance = wgpu::Instance::new(desc);
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: None,
            apply_limit_buckets: false,
        }))
        .map_err(|e| RenderError::NoAdapter(e.to_string()))?;
        let info = adapter.get_info();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("dotloom headless"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
            experimental_features: wgpu::ExperimentalFeatures::default(),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::Off,
        }))
        .map_err(|e| RenderError::Device(e.to_string()))?;
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let msaa = adapter.get_texture_format_features(format).flags.sample_count_supported(4);
        let opts = RendererOptions { sample_count: if msaa { options.sample_count } else { 1 } };
        let renderer = Renderer::new(device, queue, format, opts)?;
        Ok(Self {
            renderer,
            adapter: AdapterSummary {
                name: info.name,
                backend: format!("{:?}", info.backend).to_lowercase(),
                device_type: format!("{:?}", info.device_type),
                driver: format!("{} {}", info.driver, info.driver_info).trim().to_string(),
            },
        })
    }

    /// The adapter in use.
    #[must_use]
    pub fn adapter(&self) -> &AdapterSummary {
        &self.adapter
    }

    /// The renderer (apply deltas, set view/theme/overlay through it).
    pub fn renderer(&mut self) -> &mut Renderer {
        &mut self.renderer
    }

    /// Render the current scene at the view's device size and read it back.
    ///
    /// # Errors
    /// [`RenderError::Readback`] if mapping the result fails.
    pub fn render(&mut self) -> Result<(Image, FrameStats), RenderError> {
        let (width, height) = self.renderer.view().device_size();
        let device = self.renderer.device().clone();
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("dotloom offscreen"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.renderer.format(),
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let stats = self.renderer.render(&texture)?;
        let row = width as usize * 4;
        let padded =
            row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("dotloom readback"),
            size: (padded * height as usize) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("dotloom readback") });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded as u32),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        let queue = self.renderer_queue();
        queue.submit([enc.finish()]);
        let ok = Arc::new(AtomicBool::new(false));
        let flag = ok.clone();
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, move |r| flag.store(r.is_ok(), Ordering::SeqCst));
        device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| RenderError::Readback(e.to_string()))?;
        if !ok.load(Ordering::SeqCst) {
            return Err(RenderError::Readback("buffer mapping failed".into()));
        }
        let mut pixels = Vec::with_capacity(row * height as usize);
        {
            let view = slice.get_mapped_range().map_err(|e| RenderError::Readback(e.to_string()))?;
            for y in 0..height as usize {
                pixels.extend_from_slice(&view[y * padded..y * padded + row]);
            }
        }
        buffer.unmap();
        buffer.destroy();
        texture.destroy();
        // Premultiplied → straight alpha for PNG.
        for px in pixels.as_chunks_mut::<4>().0 {
            let a = px[3];
            if a != 0 && a != 255 {
                for c in &mut px[..3] {
                    *c = ((u32::from(*c) * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8;
                }
            }
        }
        Ok((Image { width, height, pixels }, stats))
    }

    fn renderer_queue(&self) -> wgpu::Queue {
        self.renderer.queue().clone()
    }
}

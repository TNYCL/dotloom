//! # dotloom-render-web
//!
//! WebAssembly binding of [`dotloom_render`] for browsers. The renderer runs on
//! the main thread (the engine lives in a Worker) and draws into a canvas with an
//! explicitly chosen backend:
//!
//! * `"webgpu"` — `navigator.gpu`; fails with a clear error when unavailable.
//! * `"webgl2"` — WebGL2 through wgpu's GL backend.
//!
//! There is no silent fallback: the host decides whether to try the other
//! backend, and the chosen backend is reported by [`WebRenderer::info`]. Device or
//! context loss is reported through [`WebRenderer::lost`]; the host then disposes
//! the renderer and creates a new one.
#![cfg_attr(target_arch = "wasm32", allow(unsafe_code))]

#[cfg(target_arch = "wasm32")]
mod web {
    use std::sync::{Arc, Mutex};

    use dotloom_render::scene::Overlay;
    use dotloom_render::{Grid, Renderer, RendererOptions, Theme, View};
    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::*;

    /// Protocol version of this binding (checked by the SDK).
    pub const RENDER_PROTOCOL: u32 = 1;

    fn js_err(code: &str, message: impl core::fmt::Display) -> JsValue {
        let v = serde_json::json!({ "code": code, "message": message.to_string() });
        JsValue::from_str(&v.to_string())
    }

    /// Whether `navigator.gpu` exists (WebGPU may still fail to provide an adapter).
    #[wasm_bindgen(js_name = webgpuExposed)]
    #[must_use]
    pub fn webgpu_exposed() -> bool {
        let global = js_sys::global();
        let nav = js_sys::Reflect::get(&global, &JsValue::from_str("navigator")).unwrap_or(JsValue::UNDEFINED);
        if nav.is_undefined() || nav.is_null() {
            return false;
        }
        js_sys::Reflect::get(&nav, &JsValue::from_str("gpu")).is_ok_and(|g| !g.is_undefined() && !g.is_null())
    }

    /// Binding protocol version.
    #[wasm_bindgen(js_name = renderProtocol)]
    #[must_use]
    pub fn render_protocol() -> u32 {
        RENDER_PROTOCOL
    }

    /// A renderer attached to one canvas.
    #[wasm_bindgen]
    pub struct WebRenderer {
        surface: Option<wgpu::Surface<'static>>,
        config: wgpu::SurfaceConfiguration,
        renderer: Option<Renderer>,
        backend: String,
        info: serde_json::Value,
        lost: Arc<Mutex<Option<String>>>,
        errors: Arc<Mutex<Vec<String>>>,
    }

    impl core::fmt::Debug for WebRenderer {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            f.debug_struct("WebRenderer").field("backend", &self.backend).finish_non_exhaustive()
        }
    }

    fn pick_format(formats: &[wgpu::TextureFormat]) -> Option<wgpu::TextureFormat> {
        // Colors are authored in sRGB and blended like Canvas/SVG: use a non-sRGB target.
        let preferred = [wgpu::TextureFormat::Bgra8Unorm, wgpu::TextureFormat::Rgba8Unorm];
        preferred.into_iter().find(|f| formats.contains(f)).or_else(|| formats.first().copied())
    }

    #[wasm_bindgen]
    impl WebRenderer {
        /// Create a renderer for `canvas` (`HTMLCanvasElement` or `OffscreenCanvas`)
        /// with `backend` `"webgpu"` or `"webgl2"`. `width`/`height` are the
        /// canvas size in device pixels.
        ///
        /// # Errors
        /// A JSON string `{code, message}` with code `unsupported`, `adapter`,
        /// `device`, `surface` or `invalid`.
        pub async fn create(canvas: JsValue, backend: String, width: u32, height: u32) -> Result<WebRenderer, JsValue> {
            console_error_panic_hook::set_once();
            let backends = match backend.as_str() {
                "webgpu" => {
                    if !webgpu_exposed() {
                        return Err(js_err("unsupported", "WebGPU is not available in this browser (navigator.gpu)"));
                    }
                    wgpu::Backends::BROWSER_WEBGPU
                }
                "webgl2" => wgpu::Backends::GL,
                other => return Err(js_err("invalid", format!("unknown backend `{other}` (use webgpu or webgl2)"))),
            };
            let target = if let Some(c) = canvas.dyn_ref::<web_sys::HtmlCanvasElement>() {
                wgpu::SurfaceTarget::Canvas(c.clone())
            } else if let Some(c) = canvas.dyn_ref::<web_sys::OffscreenCanvas>() {
                wgpu::SurfaceTarget::OffscreenCanvas(c.clone())
            } else {
                return Err(js_err("invalid", "expected an HTMLCanvasElement or OffscreenCanvas"));
            };
            let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
            desc.backends = backends;
            let instance = wgpu::Instance::new(desc);
            let surface = instance.create_surface(target).map_err(|e| js_err("surface", e))?;
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    force_fallback_adapter: false,
                    compatible_surface: Some(&surface),
                    apply_limit_buckets: false,
                })
                .await
                .map_err(|e| js_err("adapter", format!("no {backend} adapter: {e}")))?;
            let (device, queue) = adapter
                .request_device(&wgpu::DeviceDescriptor {
                    label: Some("dotloom"),
                    required_features: wgpu::Features::empty(),
                    // The same WebGL2-level limits on both backends keep them equivalent.
                    required_limits: wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
                    experimental_features: wgpu::ExperimentalFeatures::default(),
                    memory_hints: wgpu::MemoryHints::MemoryUsage,
                    trace: wgpu::Trace::Off,
                })
                .await
                .map_err(|e| js_err("device", e))?;
            let lost = Arc::new(Mutex::new(None));
            let lost_cb = lost.clone();
            device.set_device_lost_callback(move |reason, msg| {
                if let Ok(mut l) = lost_cb.lock() {
                    *l = Some(format!("{reason:?}: {msg}"));
                }
            });
            let errors = Arc::new(Mutex::new(Vec::new()));
            let errors_cb = errors.clone();
            device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
                if let Ok(mut v) = errors_cb.lock()
                    && v.len() < 32
                {
                    v.push(e.to_string());
                }
            }));
            let caps = surface.get_capabilities(&adapter);
            let format = pick_format(&caps.formats).ok_or_else(|| js_err("surface", "surface supports no formats"))?;
            let alpha_mode = if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::PreMultiplied) {
                wgpu::CompositeAlphaMode::PreMultiplied
            } else {
                caps.alpha_modes.first().copied().unwrap_or(wgpu::CompositeAlphaMode::Auto)
            };
            let mut config = surface
                .get_default_config(&adapter, width.max(1), height.max(1))
                .ok_or_else(|| js_err("surface", "surface is not supported by the adapter"))?;
            config.format = format;
            config.alpha_mode = alpha_mode;
            config.present_mode = wgpu::PresentMode::Fifo;
            config.view_formats = Vec::new();
            surface.configure(&device, &config);
            let msaa = adapter.get_texture_format_features(format).flags.sample_count_supported(4);
            let renderer =
                Renderer::new(device, queue, format, RendererOptions { sample_count: if msaa { 4 } else { 1 } })
                    .map_err(|e| js_err("device", e))?;
            let ai = adapter.get_info();
            let limits = adapter.limits();
            let info = serde_json::json!({
                "backend": backend,
                "wgpuBackend": format!("{:?}", ai.backend).to_lowercase(),
                "adapter": ai.name,
                "vendor": ai.vendor,
                "deviceType": format!("{:?}", ai.device_type),
                "driver": ai.driver,
                "format": format!("{format:?}"),
                "alphaMode": format!("{alpha_mode:?}"),
                "sampleCount": renderer.sample_count(),
                "maxTextureDimension2D": limits.max_texture_dimension_2d,
                "protocol": RENDER_PROTOCOL,
            });
            Ok(WebRenderer { surface: Some(surface), config, renderer: Some(renderer), backend, info, lost, errors })
        }

        /// Backend in use (`webgpu` or `webgl2`).
        #[must_use]
        pub fn backend(&self) -> String {
            self.backend.clone()
        }

        /// Adapter and surface details (JSON).
        #[must_use]
        pub fn info(&self) -> String {
            self.info.to_string()
        }

        fn r(&mut self) -> Result<&mut Renderer, JsValue> {
            self.renderer.as_mut().ok_or_else(|| js_err("disposed", "renderer disposed"))
        }

        /// Resize the drawing buffer (device pixels).
        ///
        /// # Errors
        /// When disposed.
        pub fn resize(&mut self, width: u32, height: u32) -> Result<(), JsValue> {
            let (w, h) = (width.clamp(1, 16384), height.clamp(1, 16384));
            let device = self.r()?.device().clone();
            if (w, h) != (self.config.width, self.config.height) {
                self.config.width = w;
                self.config.height = h;
                if let Some(s) = &self.surface {
                    s.configure(&device, &self.config);
                }
            }
            Ok(())
        }

        /// Set the camera: world center, CSS px per unit, CSS size and DPR.
        ///
        /// # Errors
        /// Invalid values.
        #[wasm_bindgen(js_name = setView)]
        pub fn set_view(
            &mut self,
            cx: f64,
            cy: f64,
            scale: f64,
            width: f64,
            height: f64,
            dpr: f64,
        ) -> Result<(), JsValue> {
            let v = View { center: [cx, cy], scale, width, height, dpr };
            self.r()?.set_view(v).map_err(|e| js_err("invalid", e))
        }

        /// Apply a binary scene delta.
        ///
        /// # Errors
        /// Malformed deltas (the scene is unchanged).
        #[wasm_bindgen(js_name = applyDelta)]
        pub fn apply_delta(&mut self, bytes: &[u8]) -> Result<(), JsValue> {
            self.r()?.apply_delta_bytes(bytes).map_err(|e| js_err("scene", e))
        }

        /// Set colors (JSON `Theme`; missing fields keep light-theme defaults).
        ///
        /// # Errors
        /// Invalid JSON.
        #[wasm_bindgen(js_name = setTheme)]
        pub fn set_theme(&mut self, json: &str) -> Result<(), JsValue> {
            let t: Theme = serde_json::from_str(json).map_err(|e| js_err("invalid", e))?;
            self.r()?.set_theme(t);
            Ok(())
        }

        /// Set grid settings (JSON `Grid`).
        ///
        /// # Errors
        /// Invalid JSON.
        #[wasm_bindgen(js_name = setGrid)]
        pub fn set_grid(&mut self, json: &str) -> Result<(), JsValue> {
            let g: Grid = serde_json::from_str(json).map_err(|e| js_err("invalid", e))?;
            self.r()?.set_grid(g);
            Ok(())
        }

        /// Set the interaction overlay (JSON `Overlay`).
        ///
        /// # Errors
        /// Invalid JSON.
        #[wasm_bindgen(js_name = setOverlay)]
        pub fn set_overlay(&mut self, json: &str) -> Result<(), JsValue> {
            let o: Overlay = serde_json::from_str(json).map_err(|e| js_err("invalid", e))?;
            self.r()?.set_overlay(o);
            Ok(())
        }

        /// Hovered entity (`-1` for none).
        ///
        /// # Errors
        /// When disposed.
        #[wasm_bindgen(js_name = setHover)]
        pub fn set_hover(&mut self, id: f64) -> Result<(), JsValue> {
            let h = (id >= 0.0 && id.is_finite()).then_some(id as u64);
            self.r()?.set_hover(h);
            Ok(())
        }

        /// World bounds of the scene as JSON `{min, max}` or `null`.
        ///
        /// # Errors
        /// When disposed.
        #[wasm_bindgen(js_name = sceneBounds)]
        pub fn scene_bounds(&mut self) -> Result<String, JsValue> {
            let b = self.r()?.scene().bounds();
            Ok(if b.is_empty() {
                "null".into()
            } else {
                serde_json::json!({ "min": [b.min.x, b.min.y], "max": [b.max.x, b.max.y] }).to_string()
            })
        }

        /// Draw a frame. Returns frame statistics (JSON) or `null` if the frame was
        /// skipped (surface outdated/occluded; the next call retries).
        ///
        /// # Errors
        /// `lost` when the device was lost; `gpu` for validation errors.
        pub fn render(&mut self) -> Result<String, JsValue> {
            if let Some(reason) = self.lost() {
                return Err(js_err("lost", reason));
            }
            let device = self.r()?.device().clone();
            let Some(surface) = &self.surface else { return Err(js_err("disposed", "renderer disposed")) };
            let frame = match surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
                wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                    surface.configure(&device, &self.config);
                    return Ok("null".into());
                }
                wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                    return Ok("null".into());
                }
                wgpu::CurrentSurfaceTexture::Validation => {
                    return Err(js_err("gpu", self.take_errors().unwrap_or_else(|| "surface validation error".into())));
                }
            };
            let stats = self.r()?.render(&frame.texture).map_err(|e| js_err("render", e))?;
            self.r()?.queue().present(frame);
            if let Some(e) = self.take_errors() {
                return Err(js_err("gpu", e));
            }
            serde_json::to_string(&stats).map_err(|e| js_err("render", e))
        }

        fn take_errors(&self) -> Option<String> {
            let mut v = self.errors.lock().ok()?;
            if v.is_empty() {
                return None;
            }
            Some(core::mem::take(&mut *v).join("; "))
        }

        /// Why the device was lost, if it was.
        #[must_use]
        pub fn lost(&self) -> Option<String> {
            self.lost.lock().ok().and_then(|l| l.clone())
        }

        /// Destroy the device (simulates device loss in tests).
        #[wasm_bindgen(js_name = loseDeviceForTesting)]
        pub fn lose_device_for_testing(&self) {
            if let Some(r) = &self.renderer {
                r.device().destroy();
            }
        }

        /// Release GPU resources now. The renderer cannot be used afterwards.
        pub fn dispose(&mut self) {
            if let Some(mut r) = self.renderer.take() {
                r.dispose();
            }
            self.surface = None;
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub use web::*;

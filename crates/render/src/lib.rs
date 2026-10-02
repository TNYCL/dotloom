//! # dotloom-render
//!
//! The default Dotloom renderer, built on [wgpu]. It consumes the public scene
//! contract ([`dotloom_scene`]) and never reads documents, so it can be replaced.
//!
//! * Lines are instanced, screen-constant-width capsules with analytic
//!   anti-aliasing and screen-space dash patterns.
//! * Fills are tessellated with lyon; MSAA smooths their edges when available.
//! * Text uses a signed-distance-field atlas of a bundled OFL font (Inter subset
//!   with Latin, Turkish, Greek, Cyrillic and technical symbols).
//! * Items are batched into order-preserving chunks with world bounding boxes for
//!   viewport culling; meshes are cached per item and rebuilt only when the item
//!   changes, its zoom bucket changes (curves), or the glyph atlas is reset.
//! * Only WebGL2-compatible GPU features are used, so the WebGPU and WebGL2
//!   backends run identical shaders.
//!
//! The browser binding lives in `dotloom-render-web`; native offscreen rendering
//! (PNG) is behind the `png` feature.
//!
//! [wgpu]: https://wgpu.rs/

mod cache;
pub mod color;
mod gpu;
#[cfg(feature = "png")]
mod headless;
pub mod overlay;
pub mod tess;
pub mod text;
pub mod view;

pub use cache::{CHUNK_ITEMS, MAX_CHUNK_EXTENT, SceneCache};
pub use color::Theme;
pub use dotloom_scene as scene;
pub use gpu::{ChunkBuffers, FrameStats, Renderer, RendererOptions};
#[cfg(feature = "png")]
pub use headless::{AdapterSummary, Headless, Image};
pub use overlay::Grid;
pub use view::View;
pub use wgpu;

/// Renderer errors.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RenderError {
    /// Malformed scene delta.
    #[error("invalid scene delta: {0}")]
    Scene(String),
    /// Invalid camera values.
    #[error("invalid view: {0}")]
    InvalidView(String),
    /// Font could not be loaded.
    #[error("font error: {0}")]
    Font(String),
    /// No GPU adapter for the requested backend.
    #[error("no GPU adapter available: {0}")]
    NoAdapter(String),
    /// Device creation failed.
    #[error("GPU device request failed: {0}")]
    Device(String),
    /// Surface creation/configuration failed.
    #[error("surface error: {0}")]
    Surface(String),
    /// Reading back pixels failed.
    #[error("readback failed: {0}")]
    Readback(String),
    /// The GPU device was lost; create a new renderer.
    #[error("GPU device lost: {0}")]
    DeviceLost(String),
    /// The renderer was disposed.
    #[error("renderer disposed")]
    Disposed,
}

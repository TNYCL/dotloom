//! Text: font loading, signed-distance-field glyph atlas and layout.
//!
//! Glyphs are rasterized once at [`RASTER_PX`] with `fontdue`, converted to a
//! signed distance field and packed into a single-channel atlas. The SDF stays
//! sharp from a few pixels up to large zoom levels, so text never needs to be
//! re-rasterized while zooming. Layout follows the scene contract (`Text::height`
//! is the cap-to-descender height; lines are `1.2 × height` apart) so drawn text
//! matches the engine's text boxes used for hit-testing.

use std::collections::HashMap;

use dotloom_geometry::{HAlign, Text, VAlign};

use crate::RenderError;

/// The default font: a subset of Inter Regular (SIL OFL 1.1, see `assets/`).
pub const DEFAULT_FONT: &[u8] = include_bytes!("../assets/Inter-Regular-subset.ttf");

/// Rasterization size of atlas glyphs (pixels per em).
pub const RASTER_PX: f32 = 48.0;
/// SDF spread and glyph padding in raster pixels.
pub const SPREAD: usize = 6;
/// Atlas edge length (fits WebGL2's guaranteed 2048 texture limit).
pub const ATLAS_SIZE: usize = 2048;

/// Line distance relative to the text height (scene contract).
const LINE_SPACING: f64 = Text::LINE_SPACING;
/// Ascent used for vertical alignment (scene contract: top ≈ 0.8 h above baseline).
const TOP_ABOVE_BASELINE: f64 = 0.8;

/// A glyph in the atlas.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GlyphSlot {
    /// Atlas UV rectangle `[u0, v0, u1, v1]` (v down); `None` for blank glyphs.
    pub uv: Option<[f32; 4]>,
    /// Quad in em units relative to the pen position on the baseline (y up):
    /// `[x0, y0, x1, y1]`.
    pub quad: [f32; 4],
    /// Advance in em units.
    pub advance: f32,
}

/// One positioned glyph (text-local model units before rotation, y up).
#[derive(Debug, Clone, Copy)]
pub(crate) struct PlacedGlyph {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
    pub uv: [f32; 4],
}

/// Dirty atlas region to upload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DirtyRect {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

struct Shelf {
    y: usize,
    h: usize,
    x: usize,
}

/// Font, atlas and glyph cache.
pub struct TextSystem {
    font: fontdue::Font,
    /// Text height → em size factor: `em = height / height_per_em`.
    height_per_em: f64,
    glyphs: HashMap<u16, GlyphSlot>,
    pub(crate) atlas: Vec<u8>,
    shelves: Vec<Shelf>,
    pub(crate) dirty: Option<DirtyRect>,
    /// Incremented when the atlas is cleared; cached text meshes become stale.
    pub(crate) generation: u64,
}

impl std::fmt::Debug for TextSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextSystem").field("glyphs", &self.glyphs.len()).field("generation", &self.generation).finish()
    }
}

impl TextSystem {
    /// Load a TrueType/OpenType font.
    ///
    /// # Errors
    /// [`RenderError::Font`] when the font cannot be parsed or lacks metrics.
    pub fn new(font_bytes: &[u8]) -> Result<Self, RenderError> {
        let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
            .map_err(|e| RenderError::Font(e.to_string()))?;
        let upm = font.units_per_em();
        let lm = font
            .horizontal_line_metrics(upm)
            .ok_or_else(|| RenderError::Font("font has no horizontal metrics".into()))?;
        let h = font.metrics('H', upm);
        let cap = if h.height > 0 { (h.ymin as f32 + h.height as f32) / upm } else { lm.ascent / upm * 0.75 };
        let descent = (-lm.descent / upm).max(0.0);
        let height_per_em = f64::from(cap + descent);
        if !(height_per_em.is_finite() && height_per_em > 0.1) {
            return Err(RenderError::Font("implausible font metrics".into()));
        }
        Ok(Self {
            font,
            height_per_em,
            glyphs: HashMap::new(),
            atlas: vec![0; ATLAS_SIZE * ATLAS_SIZE],
            shelves: Vec::new(),
            dirty: None,
            generation: 0,
        })
    }

    /// The built-in font.
    ///
    /// # Errors
    /// Never for the bundled font; see [`TextSystem::new`].
    pub fn with_default_font() -> Result<Self, RenderError> {
        Self::new(DEFAULT_FONT)
    }

    /// Number of glyphs in the atlas.
    #[must_use]
    pub fn cached_glyphs(&self) -> usize {
        self.glyphs.len()
    }

    fn clear_atlas(&mut self) {
        self.glyphs.clear();
        self.shelves.clear();
        self.atlas.fill(0);
        self.dirty = Some(DirtyRect { x: 0, y: 0, w: ATLAS_SIZE, h: ATLAS_SIZE });
        self.generation += 1;
    }

    fn mark_dirty(&mut self, r: DirtyRect) {
        self.dirty = Some(match self.dirty {
            None => r,
            Some(d) => {
                let x0 = d.x.min(r.x);
                let y0 = d.y.min(r.y);
                let x1 = (d.x + d.w).max(r.x + r.w);
                let y1 = (d.y + d.h).max(r.y + r.h);
                DirtyRect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
            }
        });
    }

    fn allocate(&mut self, w: usize, h: usize) -> Option<(usize, usize)> {
        if w > ATLAS_SIZE || h > ATLAS_SIZE {
            return None;
        }
        for s in &mut self.shelves {
            if h <= s.h && s.x + w <= ATLAS_SIZE {
                let at = (s.x, s.y);
                s.x += w + 1;
                return Some(at);
            }
        }
        let y = self.shelves.last().map_or(0, |s| s.y + s.h + 1);
        if y + h > ATLAS_SIZE {
            return None;
        }
        self.shelves.push(Shelf { y, h, x: w + 1 });
        Some((0, y))
    }

    fn glyph_index(&self, c: char) -> u16 {
        if self.font.has_glyph(c) {
            return self.font.lookup_glyph_index(c);
        }
        let substitute = match c {
            '\u{2300}' => Some('\u{2205}'),
            '\u{00a0}' | '\u{2007}' | '\u{202f}' => Some(' '),
            _ => None,
        };
        match substitute {
            Some(s) if self.font.has_glyph(s) => self.font.lookup_glyph_index(s),
            _ if self.font.has_glyph('\u{fffd}') => self.font.lookup_glyph_index('\u{fffd}'),
            _ => 0,
        }
    }

    /// Get or rasterize a glyph. Returns `None` only when the atlas is full even
    /// after clearing it (more distinct glyphs than fit in one atlas).
    fn glyph(&mut self, index: u16) -> Option<GlyphSlot> {
        if let Some(g) = self.glyphs.get(&index) {
            return Some(*g);
        }
        let (m, coverage) = self.font.rasterize_indexed(index, RASTER_PX);
        let advance = m.advance_width / RASTER_PX;
        if m.width == 0 || m.height == 0 {
            let slot = GlyphSlot { uv: None, quad: [0.0; 4], advance };
            self.glyphs.insert(index, slot);
            return Some(slot);
        }
        let (w, h) = (m.width + 2 * SPREAD, m.height + 2 * SPREAD);
        let at = match self.allocate(w, h) {
            Some(at) => at,
            None => {
                self.clear_atlas();
                self.allocate(w, h)?
            }
        };
        let sdf = signed_distance_field(&coverage, m.width, m.height, SPREAD);
        for row in 0..h {
            let dst = (at.1 + row) * ATLAS_SIZE + at.0;
            self.atlas[dst..dst + w].copy_from_slice(&sdf[row * w..(row + 1) * w]);
        }
        self.mark_dirty(DirtyRect { x: at.0, y: at.1, w, h });
        let size = ATLAS_SIZE as f32;
        let pad = SPREAD as f32;
        let slot = GlyphSlot {
            uv: Some([at.0 as f32 / size, at.1 as f32 / size, (at.0 + w) as f32 / size, (at.1 + h) as f32 / size]),
            quad: [
                (m.xmin as f32 - pad) / RASTER_PX,
                (m.ymin as f32 - pad) / RASTER_PX,
                (m.xmin as f32 + m.width as f32 + pad) / RASTER_PX,
                (m.ymin as f32 + m.height as f32 + pad) / RASTER_PX,
            ],
            advance,
        };
        self.glyphs.insert(index, slot);
        Some(slot)
    }

    /// Measure the advance width of one line in model units.
    pub(crate) fn line_width(&mut self, line: &str, height: f64) -> f64 {
        let em = height / self.height_per_em;
        line.chars()
            .filter_map(|c| self.glyph(self.glyph_index(normalize_char(c)?)))
            .map(|g| f64::from(g.advance))
            .sum::<f64>()
            * em
    }

    /// Lay out text into glyph quads (text-local coordinates, y up, before
    /// rotation and translation). Returns `None` if the atlas overflowed while
    /// laying out (caller retries after the generation change).
    pub(crate) fn layout(&mut self, t: &Text) -> Option<Vec<PlacedGlyph>> {
        let h = t.height;
        if !(h.is_finite() && h > 0.0) {
            return Some(Vec::new());
        }
        let em = h / self.height_per_em;
        let lines: Vec<&str> = t.content.split('\n').collect();
        let n = lines.len().max(1) as f64;
        let block_h = h * (1.0 + (n - 1.0) * LINE_SPACING);
        let first_baseline = match t.valign {
            VAlign::Baseline => 0.0,
            VAlign::Top => -TOP_ABOVE_BASELINE * h,
            VAlign::Middle => block_h * 0.5 - TOP_ABOVE_BASELINE * h,
            VAlign::Bottom => block_h - TOP_ABOVE_BASELINE * h,
        };
        let generation = self.generation;
        let mut out = Vec::with_capacity(t.content.len());
        for (i, line) in lines.iter().enumerate() {
            let line = line.trim_end_matches('\r');
            let width = self.line_width(line, h);
            let mut pen = match t.halign {
                HAlign::Left => 0.0,
                HAlign::Center => -width * 0.5,
                HAlign::Right => -width,
            };
            let baseline = first_baseline - i as f64 * LINE_SPACING * h;
            for c in line.chars() {
                let Some(c) = normalize_char(c) else { continue };
                let g = self.glyph(self.glyph_index(c))?;
                if self.generation != generation {
                    return None;
                }
                if let Some(uv) = g.uv {
                    out.push(PlacedGlyph {
                        x0: pen + f64::from(g.quad[0]) * em,
                        y0: baseline + f64::from(g.quad[1]) * em,
                        x1: pen + f64::from(g.quad[2]) * em,
                        y1: baseline + f64::from(g.quad[3]) * em,
                        uv,
                    });
                }
                pen += f64::from(g.advance) * em;
            }
        }
        Some(out)
    }
}

/// Map control characters: tabs become spaces, other controls are dropped.
fn normalize_char(c: char) -> Option<char> {
    match c {
        '\t' => Some(' '),
        c if c.is_control() => None,
        c => Some(c),
    }
}

/// 1-D squared Euclidean distance transform (Felzenszwalb & Huttenlocher).
fn edt_1d(f: &[f32], d: &mut [f32], v: &mut [usize], z: &mut [f32]) {
    let n = f.len();
    if n == 0 {
        return;
    }
    let mut k = 0usize;
    v[0] = 0;
    z[0] = f32::NEG_INFINITY;
    z[1] = f32::INFINITY;
    for q in 1..n {
        let fq = f[q] + (q * q) as f32;
        // z[0] is -∞, so the loop always stops at k = 0.
        let mut s;
        loop {
            let p = v[k];
            s = (fq - (f[p] + (p * p) as f32)) / (2.0 * q as f32 - 2.0 * p as f32);
            if s <= z[k] && k > 0 {
                k -= 1;
            } else {
                break;
            }
        }
        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = f32::INFINITY;
    }
    k = 0;
    for (q, out) in d.iter_mut().enumerate().take(n) {
        while z[k + 1] < q as f32 {
            k += 1;
        }
        let p = v[k];
        let dq = q as f32 - p as f32;
        *out = dq * dq + f[p];
    }
}

/// 2-D squared distance to the nearest pixel where `feature` is true.
fn edt_2d(feature: &[bool], w: usize, h: usize) -> Vec<f32> {
    const INF: f32 = 1e20;
    let mut grid: Vec<f32> = feature.iter().map(|&b| if b { 0.0 } else { INF }).collect();
    let n = w.max(h);
    let (mut f, mut d) = (vec![0.0f32; n], vec![0.0f32; n]);
    let (mut v, mut z) = (vec![0usize; n], vec![0.0f32; n + 1]);
    for x in 0..w {
        for y in 0..h {
            f[y] = grid[y * w + x];
        }
        edt_1d(&f[..h], &mut d[..h], &mut v, &mut z);
        for y in 0..h {
            grid[y * w + x] = d[y];
        }
    }
    for y in 0..h {
        f[..w].copy_from_slice(&grid[y * w..(y + 1) * w]);
        edt_1d(&f[..w], &mut d[..w], &mut v, &mut z);
        grid[y * w..(y + 1) * w].copy_from_slice(&d[..w]);
    }
    grid
}

/// Signed distance field of a coverage bitmap, padded by `spread` on each side.
/// Encoded as `0.5` on the outline, increasing inside; `spread` pixels map to ±0.5.
pub(crate) fn signed_distance_field(coverage: &[u8], w: usize, h: usize, spread: usize) -> Vec<u8> {
    let (pw, ph) = (w + 2 * spread, h + 2 * spread);
    let mut cov = vec![0u8; pw * ph];
    for y in 0..h {
        let dst = (y + spread) * pw + spread;
        cov[dst..dst + w].copy_from_slice(&coverage[y * w..(y + 1) * w]);
    }
    let inside: Vec<bool> = cov.iter().map(|&c| c >= 128).collect();
    let outside: Vec<bool> = inside.iter().map(|&b| !b).collect();
    let to_inside = edt_2d(&inside, pw, ph);
    let to_outside = edt_2d(&outside, pw, ph);
    let s = spread as f32;
    cov.iter()
        .enumerate()
        .map(|(i, &c)| {
            // Signed distance in pixels, positive inside.
            let sd = if c > 0 && c < 255 {
                f32::from(c) / 255.0 - 0.5
            } else if inside[i] {
                to_outside[i].sqrt() - 0.5
            } else {
                -(to_inside[i].sqrt() - 0.5)
            };
            ((0.5 + sd / (2.0 * s)).clamp(0.0, 1.0) * 255.0).round() as u8
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use dotloom_geometry::Point;

    use super::*;

    fn text(s: &str) -> Text {
        Text {
            position: Point::new(0.0, 0.0),
            content: s.into(),
            height: 10.0,
            rotation: 0.0,
            halign: HAlign::Left,
            valign: VAlign::Baseline,
        }
    }

    #[test]
    fn turkish_and_symbols_have_glyphs() {
        let ts = TextSystem::with_default_font().unwrap();
        for c in "çÇğĞıİöÖşŞüÜ°±×∅µ²€ΩπДж".chars() {
            assert!(ts.font.has_glyph(c), "missing {c}");
        }
        // Diameter sign falls back to the empty-set glyph.
        assert_eq!(ts.glyph_index('\u{2300}'), ts.glyph_index('\u{2205}'));
    }

    #[test]
    fn layout_metrics_follow_the_contract() {
        let mut ts = TextSystem::with_default_font().unwrap();
        let g = ts.layout(&text("H")).unwrap();
        assert_eq!(g.len(), 1);
        // Cap height of Inter ≈ 0.75 × text height (cap + descender = height).
        let cap_top = g[0].y1 - (SPREAD as f64 / f64::from(RASTER_PX)) * (10.0 / ts.height_per_em);
        assert!((cap_top - 7.5).abs() < 0.3, "cap top {cap_top}");
        // Second line is 1.2 h lower.
        let two = ts.layout(&text("H\nH")).unwrap();
        assert!((two[0].y0 - two[1].y0 - 12.0).abs() < 1e-9);
    }

    #[test]
    fn alignment() {
        let mut ts = TextSystem::with_default_font().unwrap();
        let mut t = text("İstanbul");
        let w = ts.line_width(&t.content, t.height);
        assert!(w > 20.0 && w < 60.0, "{w}");
        t.halign = HAlign::Right;
        let g = ts.layout(&t).unwrap();
        // Quads include the SDF padding (SPREAD / RASTER_PX em ≈ 1.3 units here).
        let pad = SPREAD as f64 / f64::from(RASTER_PX) * (t.height / ts.height_per_em);
        let right = g.last().unwrap().x1 - pad;
        assert!(right <= 0.1 && right > -2.0, "{right}");
        t.halign = HAlign::Center;
        t.valign = VAlign::Middle;
        let g = ts.layout(&t).unwrap();
        let min_x = g.iter().map(|q| q.x0).fold(f64::INFINITY, f64::min);
        assert!((min_x + w * 0.5).abs() < 1.5);
    }

    #[test]
    fn sdf_is_monotonic_across_an_edge() {
        // A filled square in a 10×10 bitmap.
        let mut cov = vec![0u8; 100];
        for y in 3..7 {
            for x in 3..7 {
                cov[y * 10 + x] = 255;
            }
        }
        let sdf = signed_distance_field(&cov, 10, 10, 4);
        let pw = 18;
        let row = 4 + 5;
        let vals: Vec<u8> = (0..pw).map(|x| sdf[row * pw + x]).collect();
        let center = vals[9];
        assert!(center > 128, "{vals:?}");
        assert!(vals[0] < 64);
        for x in 1..9 {
            assert!(vals[x] >= vals[x - 1], "{vals:?}");
        }
    }

    #[test]
    fn controls_are_skipped_and_empty_text_is_fine() {
        let mut ts = TextSystem::with_default_font().unwrap();
        assert!(ts.layout(&text("")).unwrap().is_empty());
        assert_eq!(ts.layout(&text("a\u{7}b")).unwrap().len(), 2);
        let mut bad = text("x");
        bad.height = f64::NAN;
        assert!(ts.layout(&bad).unwrap().is_empty());
    }
}

//! Renderer-side scene cache: items, draw order, chunked batches.
//!
//! Items are kept in draw order (layer, then the scene's order) and grouped into
//! consecutive *chunks*. Each chunk is one set of GPU buffers with a world-space
//! bounding box used for viewport culling. Inside a chunk, geometry is grouped by
//! kind (fills, lines, glyphs) into *runs*: a new run starts only when an item
//! would otherwise be drawn below something that precedes it and overlaps it, so
//! batching never changes the visible stacking order.
//!
//! Item meshes are cached and only re-tessellated when the item changes, when a
//! curve item is drawn at a different zoom bucket, or when the glyph atlas is reset.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::ops::Range;

use dotloom_geometry::Aabb;
use dotloom_scene::{SceneDelta, SceneItem};

use crate::color::Theme;
use crate::gpu::ChunkBuffers;
use crate::tess::{
    FillVertex, GlyphInstance, LineInstance, Look, Mesh, item_has_curves, item_has_text, tessellate_item,
};
use crate::text::TextSystem;

/// Maximum items per chunk.
pub const CHUNK_ITEMS: usize = 256;
/// Maximum world extent of a chunk (keeps `f32` offsets precise to ~8 µm).
pub const MAX_CHUNK_EXTENT: f64 = 1.0e5;

struct Entry {
    item: SceneItem,
    version: u64,
    mesh: Option<CachedMesh>,
    curves: bool,
    text: bool,
}

struct CachedMesh {
    mesh: Mesh,
    lod: i32,
    text_gen: u64,
}

/// A batch of draws that preserves stacking order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Run {
    /// Fill index range.
    pub fills: Range<u32>,
    /// Line instance range.
    pub lines: Range<u32>,
    /// Glyph instance range.
    pub glyphs: Range<u32>,
}

/// CPU geometry of a chunk.
#[derive(Debug, Default)]
pub(crate) struct ChunkData {
    pub origin: [f64; 2],
    pub lines: Vec<LineInstance>,
    pub fill_vertices: Vec<FillVertex>,
    pub fill_indices: Vec<u32>,
    pub glyphs: Vec<GlyphInstance>,
    pub runs: Vec<Run>,
}

/// One chunk of consecutive items.
pub(crate) struct Chunk {
    pub ids: Vec<u64>,
    pub bbox: Aabb,
    /// Contains items without a finite bounding box (never culled).
    pub unbounded: bool,
    /// Identity of the content (IDs and versions).
    pub key: u64,
    pub curves: bool,
    pub text: bool,
    /// Signature of the data currently built (`key`, lod, text generation, theme epoch).
    pub built: Option<(u64, i32, u64, u64)>,
    pub data: Option<ChunkData>,
    /// GPU buffers and the signature they hold.
    pub gpu: ChunkBuffers,
    pub uploaded: Option<(u64, i32, u64, u64)>,
}

impl Chunk {
    fn new(ids: Vec<u64>, key: u64, bbox: Aabb, unbounded: bool, curves: bool, text: bool) -> Self {
        Self {
            ids,
            bbox,
            unbounded,
            key,
            curves,
            text,
            built: None,
            data: None,
            gpu: ChunkBuffers::default(),
            uploaded: None,
        }
    }

    fn wanted(&self, lod: i32, text_gen: u64, epoch: u64) -> (u64, i32, u64, u64) {
        (self.key, if self.curves { lod } else { 0 }, if self.text { text_gen } else { 0 }, epoch)
    }
}

/// Per-frame inputs of [`SceneCache::prepare`].
#[derive(Clone, Copy)]
pub(crate) struct FrameParams<'a> {
    /// Visible world rectangle.
    pub visible: Aabb,
    /// Extra world margin around it.
    pub margin: f64,
    /// Zoom bucket.
    pub lod: i32,
    /// Flatten tolerance of the bucket.
    pub lod_tol: f64,
    /// Colors.
    pub theme: &'a Theme,
}

/// Counters for diagnostics.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct PrepareStats {
    pub rebuilt_chunks: u32,
    pub tessellated_items: u32,
}

/// Scene state held by the renderer.
#[derive(Default)]
pub struct SceneCache {
    entries: HashMap<u64, Entry>,
    order: Vec<u64>,
    next_version: u64,
    revision: u64,
    pub(crate) chunks: Vec<Chunk>,
    chunk_of: HashMap<u64, usize>,
    partition_dirty: bool,
    /// Bumped on theme changes (all meshes stale).
    epoch: u64,
}

impl std::fmt::Debug for SceneCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SceneCache")
            .field("items", &self.entries.len())
            .field("chunks", &self.chunks.len())
            .field("revision", &self.revision)
            .finish()
    }
}

fn finite_box(b: Aabb) -> Option<Aabb> {
    (!b.is_empty() && b.min.is_finite() && b.max.is_finite()).then_some(b)
}

impl SceneCache {
    /// Number of items.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the scene is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Revision of the last applied committed delta.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// An item by ID.
    #[must_use]
    pub fn item(&self, id: u64) -> Option<&SceneItem> {
        self.entries.get(&id).map(|e| &e.item)
    }

    /// Union of all item boxes.
    #[must_use]
    pub fn bounds(&self) -> Aabb {
        self.entries.values().filter_map(|e| finite_box(e.item.bbox)).fold(Aabb::EMPTY, Aabb::union)
    }

    /// Apply a scene delta.
    pub fn apply(&mut self, delta: SceneDelta) {
        if delta.reset {
            self.entries.clear();
            self.order.clear();
            self.partition_dirty = true;
        }
        for id in &delta.removals {
            if self.entries.remove(id).is_some() {
                self.partition_dirty = true;
            }
        }
        for item in delta.upserts {
            self.next_version += 1;
            let id = item.id;
            let curves = item_has_curves(&item);
            let text = item_has_text(&item);
            let layer_changed = self.entries.get(&id).is_none_or(|e| e.item.layer != item.layer);
            if layer_changed {
                self.partition_dirty = true;
            }
            // Box changes can move the chunk's culling box; content key changes.
            if !self.partition_dirty
                && let Some(&ci) = self.chunk_of.get(&id)
                && let Some(c) = self.chunks.get_mut(ci)
            {
                c.key = 0;
            }
            self.entries.insert(id, Entry { item, version: self.next_version, mesh: None, curves, text });
        }
        if let Some(order) = delta.order {
            self.order = order;
            self.partition_dirty = true;
        }
        if !delta.preview {
            self.revision = delta.revision;
        }
        if !self.partition_dirty && self.chunks.iter().any(|c| c.key == 0) {
            // Recompute keys/boxes of touched chunks without re-partitioning.
            for ci in 0..self.chunks.len() {
                if self.chunks[ci].key == 0 {
                    self.refresh_chunk(ci);
                }
            }
        }
    }

    /// Theme changed: every mesh must be rebuilt.
    pub(crate) fn invalidate_all(&mut self) {
        self.epoch += 1;
        for e in self.entries.values_mut() {
            e.mesh = None;
        }
    }

    fn refresh_chunk(&mut self, ci: usize) {
        let Some(c) = self.chunks.get(ci) else { return };
        let ids = c.ids.clone();
        let (key, bbox, unbounded, curves, text) = self.describe(&ids);
        if let Some(c) = self.chunks.get_mut(ci) {
            c.key = key;
            c.bbox = bbox;
            c.unbounded = unbounded;
            c.curves = curves;
            c.text = text;
        }
    }

    fn describe(&self, ids: &[u64]) -> (u64, Aabb, bool, bool, bool) {
        let mut h = DefaultHasher::new();
        let mut bbox = Aabb::EMPTY;
        let mut unbounded = false;
        let (mut curves, mut text) = (false, false);
        for id in ids {
            id.hash(&mut h);
            if let Some(e) = self.entries.get(id) {
                e.version.hash(&mut h);
                match finite_box(e.item.bbox) {
                    Some(b) => bbox = bbox.union(b),
                    None => unbounded |= !e.item.prims.is_empty(),
                }
                curves |= e.curves;
                text |= e.text;
            }
        }
        // 0 is reserved for "stale".
        (h.finish().max(1), bbox, unbounded, curves, text)
    }

    fn sorted_ids(&self) -> Vec<u64> {
        let pos: HashMap<u64, usize> = self.order.iter().enumerate().map(|(i, id)| (*id, i)).collect();
        let mut ids: Vec<u64> = self.entries.keys().copied().collect();
        ids.sort_by_key(|id| {
            let layer = self.entries.get(id).map_or(u32::MAX, |e| e.item.layer);
            (layer, pos.get(id).copied().unwrap_or(usize::MAX), *id)
        });
        ids
    }

    fn repartition(&mut self) {
        let ids = self.sorted_ids();
        let mut groups: Vec<Vec<u64>> = Vec::new();
        let mut cur: Vec<u64> = Vec::new();
        let mut cur_box = Aabb::EMPTY;
        for id in ids {
            let b = self.entries.get(&id).and_then(|e| finite_box(e.item.bbox));
            let grown = b.map_or(cur_box, |b| cur_box.union(b));
            let too_wide = !grown.is_empty() && grown.width().max(grown.height()) > MAX_CHUNK_EXTENT;
            if !cur.is_empty() && (cur.len() >= CHUNK_ITEMS || too_wide) {
                groups.push(core::mem::take(&mut cur));
                cur_box = Aabb::EMPTY;
            }
            if let Some(b) = b {
                cur_box = cur_box.union(b);
            }
            cur.push(id);
        }
        if !cur.is_empty() {
            groups.push(cur);
        }
        let mut old: HashMap<u64, Chunk> = self.chunks.drain(..).map(|c| (c.key, c)).collect();
        let mut chunks = Vec::with_capacity(groups.len());
        for g in groups {
            let (key, bbox, unbounded, curves, text) = self.describe(&g);
            match old.remove(&key) {
                Some(mut c) if c.ids == g => {
                    c.bbox = bbox;
                    c.unbounded = unbounded;
                    chunks.push(c);
                }
                _ => chunks.push(Chunk::new(g, key, bbox, unbounded, curves, text)),
            }
        }
        // Reuse GPU buffers of dropped chunks for new ones (avoids reallocations).
        let mut spare: Vec<ChunkBuffers> = old.into_values().map(|c| c.gpu).collect();
        for c in &mut chunks {
            if c.uploaded.is_none()
                && c.gpu.is_empty()
                && let Some(b) = spare.pop()
            {
                c.gpu = b;
            }
        }
        for b in spare {
            b.destroy();
        }
        self.chunk_of.clear();
        for (ci, c) in chunks.iter().enumerate() {
            for id in &c.ids {
                self.chunk_of.insert(*id, ci);
            }
        }
        self.chunks = chunks;
        self.partition_dirty = false;
    }

    fn mesh_ok(e: &Entry, lod: i32, text_gen: u64) -> bool {
        e.mesh.as_ref().is_some_and(|m| (!e.curves || m.lod == lod) && (!e.text || m.text_gen == text_gen))
    }

    /// Make sure visible chunks have up-to-date CPU data. Returns visible chunk
    /// indices in draw order.
    pub(crate) fn prepare(
        &mut self,
        fp: &FrameParams<'_>,
        ts: &mut TextSystem,
        stats: &mut PrepareStats,
    ) -> Vec<usize> {
        let FrameParams { visible, margin, lod, lod_tol, theme } = *fp;
        if self.partition_dirty {
            self.repartition();
        }
        let view = visible.inflate(margin);
        let vis: Vec<usize> = (0..self.chunks.len())
            .filter(|&ci| {
                let c = &self.chunks[ci];
                c.unbounded || (!c.bbox.is_empty() && c.bbox.intersects(view))
            })
            .collect();
        // The atlas may be reset while laying out text; retry once with the new generation.
        for _ in 0..3 {
            let text_gen = ts.generation;
            let mut reset = false;
            for &ci in &vis {
                let want = self.chunks[ci].wanted(lod, text_gen, self.epoch);
                if self.chunks[ci].built == Some(want) {
                    continue;
                }
                let ids = self.chunks[ci].ids.clone();
                for id in &ids {
                    let Some(e) = self.entries.get_mut(id) else { continue };
                    if Self::mesh_ok(e, lod, text_gen) {
                        continue;
                    }
                    match tessellate_item(&e.item, Look { flags: e.item.flags }, lod_tol, theme, ts) {
                        Some(mesh) => {
                            e.mesh = Some(CachedMesh { mesh, lod, text_gen });
                            stats.tessellated_items += 1;
                        }
                        None => {
                            reset = true;
                            break;
                        }
                    }
                }
                if reset {
                    break;
                }
                let data = self.assemble(&ids);
                let c = &mut self.chunks[ci];
                c.data = Some(data);
                c.built = Some(want);
                stats.rebuilt_chunks += 1;
            }
            if !reset {
                break;
            }
        }
        vis
    }

    fn assemble(&self, ids: &[u64]) -> ChunkData {
        let meshes: Vec<(&Mesh, Aabb)> = ids
            .iter()
            .filter_map(|id| self.entries.get(id))
            .filter_map(|e| e.mesh.as_ref().map(|m| (&m.mesh, e.item.bbox)))
            .collect();
        let origin = {
            let b = meshes.iter().filter_map(|(_, b)| finite_box(*b)).fold(Aabb::EMPTY, Aabb::union);
            if b.is_empty() {
                meshes.first().map_or([0.0, 0.0], |(m, _)| m.origin)
            } else {
                let c = b.center();
                [c.x, c.y]
            }
        };
        let mut d = ChunkData { origin, ..ChunkData::default() };
        let mut run = Run::default();
        // Boxes of items in the current run that drew lines/glyphs, and glyphs only.
        let mut line_or_glyph: Vec<Aabb> = Vec::new();
        let mut glyph_boxes: Vec<Aabb> = Vec::new();
        let overlaps = |list: &[Aabb], b: Option<Aabb>| match b {
            Some(b) => list.iter().any(|x| x.intersects(b)),
            None => !list.is_empty(),
        };
        for (m, bbox) in meshes {
            if m.is_empty() {
                continue;
            }
            let b = finite_box(bbox).map(|b| b.inflate(1e-9));
            let has_fill = !m.fill_indices.is_empty();
            let has_line = !m.lines.is_empty();
            let has_glyph = !m.glyphs.is_empty();
            let split = (has_fill && overlaps(&line_or_glyph, b)) || (has_line && overlaps(&glyph_boxes, b));
            if split {
                d.runs.push(core::mem::take(&mut run));
                let (f, l, g) = (d.fill_indices.len() as u32, d.lines.len() as u32, d.glyphs.len() as u32);
                run = Run { fills: f..f, lines: l..l, glyphs: g..g };
                line_or_glyph.clear();
                glyph_boxes.clear();
            }
            let off = [(m.origin[0] - origin[0]) as f32, (m.origin[1] - origin[1]) as f32];
            let add = |p: [f32; 2]| [p[0] + off[0], p[1] + off[1]];
            let base = d.fill_vertices.len() as u32;
            d.fill_vertices.extend(m.fill_vertices.iter().map(|v| FillVertex { pos: add(v.pos), ..*v }));
            d.fill_indices.extend(m.fill_indices.iter().map(|i| i + base));
            d.lines.extend(m.lines.iter().map(|l| LineInstance { p0: add(l.p0), p1: add(l.p1), ..*l }));
            d.glyphs.extend(m.glyphs.iter().map(|g| GlyphInstance { origin: add(g.origin), ..*g }));
            run.fills.end = d.fill_indices.len() as u32;
            run.lines.end = d.lines.len() as u32;
            run.glyphs.end = d.glyphs.len() as u32;
            if let Some(b) = b {
                if has_line || has_glyph {
                    line_or_glyph.push(b);
                }
                if has_glyph {
                    glyph_boxes.push(b);
                }
            } else {
                // Unknown extent: be conservative.
                if has_line || has_glyph {
                    line_or_glyph.push(Aabb::from_corners(
                        dotloom_geometry::Point::new(f64::MIN, f64::MIN),
                        dotloom_geometry::Point::new(f64::MAX, f64::MAX),
                    ));
                }
            }
        }
        if run != Run::default() {
            d.runs.push(run);
        }
        d.runs.retain(|r| !(r.fills.is_empty() && r.lines.is_empty() && r.glyphs.is_empty()));
        d
    }

    /// Release GPU buffers of all chunks.
    pub(crate) fn destroy_gpu(&mut self) {
        for c in &mut self.chunks {
            core::mem::take(&mut c.gpu).destroy();
            c.uploaded = None;
        }
    }

    /// IDs in draw order (for tests and diagnostics).
    #[must_use]
    pub fn draw_order(&mut self) -> Vec<u64> {
        if self.partition_dirty {
            self.repartition();
        }
        self.chunks.iter().flat_map(|c| c.ids.iter().copied()).collect()
    }

    /// Chunk IDs (for tests).
    #[cfg(test)]
    pub(crate) fn chunk_ids(&mut self) -> Vec<Vec<u64>> {
        if self.partition_dirty {
            self.repartition();
        }
        self.chunks.iter().map(|c| c.ids.clone()).collect()
    }

    /// Number of distinct chunk keys (used to check reuse).
    #[cfg(test)]
    pub(crate) fn keys(&self) -> std::collections::HashSet<u64> {
        self.chunks.iter().map(|c| c.key).collect()
    }
}

#[cfg(test)]
mod tests {
    use dotloom_geometry::{Point, Rect, Segment, Shape};
    use dotloom_scene::{Primitive, Stroke};

    use super::*;

    fn line_item(id: u64, x: f64, layer: u32) -> SceneItem {
        let shape = Shape::Line(Segment::new(Point::new(x, 0.0), Point::new(x + 1.0, 0.0)));
        SceneItem {
            id,
            layer,
            bbox: shape.bbox(),
            flags: 0,
            prims: vec![Primitive::Shape {
                shape,
                stroke: Some(Stroke { color: 0, width: 1.0, dash: vec![] }),
                fill: None,
            }],
        }
    }

    fn rect_item(id: u64, x: f64, fill: bool, stroke: bool) -> SceneItem {
        let shape = Shape::Rect(Rect { origin: Point::new(x, 0.0), width: 10.0, height: 10.0 });
        SceneItem {
            id,
            layer: 0,
            bbox: shape.bbox(),
            flags: 0,
            prims: vec![Primitive::Shape {
                shape,
                stroke: stroke.then(|| Stroke { color: 0, width: 1.0, dash: vec![] }),
                fill: fill.then_some(0xff00_00ff),
            }],
        }
    }

    fn delta(upserts: Vec<SceneItem>, order: Option<Vec<u64>>) -> SceneDelta {
        SceneDelta { revision: 1, preview: false, reset: false, upserts, removals: vec![], order }
    }

    fn prepare(c: &mut SceneCache) -> Vec<usize> {
        let mut ts = TextSystem::with_default_font().unwrap();
        let mut st = PrepareStats::default();
        let all = Aabb::from_corners(Point::new(-1e7, -1e7), Point::new(1e7, 1e7));
        c.prepare(
            &FrameParams { visible: all, margin: 0.0, lod: 0, lod_tol: 0.1, theme: &Theme::light() },
            &mut ts,
            &mut st,
        )
    }

    #[test]
    fn order_follows_layer_then_scene_order() {
        let mut c = SceneCache::default();
        c.apply(delta(vec![line_item(1, 0.0, 1), line_item(2, 0.0, 0), line_item(3, 0.0, 0)], Some(vec![1, 3, 2])));
        assert_eq!(c.draw_order(), vec![3, 2, 1]);
    }

    #[test]
    fn chunks_split_by_count_and_reuse_unchanged_chunks() {
        let mut c = SceneCache::default();
        let n = CHUNK_ITEMS as u64 * 2 + 10;
        let items: Vec<SceneItem> = (1..=n).map(|i| line_item(i, i as f64, 0)).collect();
        c.apply(delta(items, Some((1..=n).collect())));
        let ids = c.chunk_ids();
        assert_eq!(ids.len(), 3);
        let before = c.keys();
        // Appending an item only changes the last chunk.
        c.apply(delta(vec![line_item(n + 1, 0.0, 0)], Some((1..=n + 1).collect())));
        c.chunk_ids();
        let after = c.keys();
        assert_eq!(before.intersection(&after).count(), 2);
        // Updating one item changes exactly one chunk key without repartitioning.
        c.apply(delta(vec![line_item(5, 99.0, 0)], None));
        let changed = c.keys();
        assert_eq!(after.intersection(&changed).count(), 2);
    }

    #[test]
    fn far_apart_items_get_separate_chunks() {
        let mut c = SceneCache::default();
        c.apply(delta(vec![line_item(1, 0.0, 0), line_item(2, 1.0e6, 0)], Some(vec![1, 2])));
        assert_eq!(c.chunk_ids().len(), 2);
    }

    #[test]
    fn runs_preserve_stacking() {
        let mut c = SceneCache::default();
        // 1: outlined rect, 2: filled rect overlapping 1 → its fill must come after 1's lines.
        // 3: filled rect far away → can join the same run.
        c.apply(delta(
            vec![rect_item(1, 0.0, false, true), rect_item(2, 5.0, true, false), rect_item(3, 100.0, true, false)],
            Some(vec![1, 2, 3]),
        ));
        let vis = prepare(&mut c);
        assert_eq!(vis, vec![0]);
        let d = c.chunks[0].data.as_ref().unwrap();
        assert_eq!(d.runs.len(), 2, "{:?}", d.runs);
        assert_eq!(d.runs[0].lines.len(), 4);
        assert!(d.runs[0].fills.is_empty());
        assert_eq!(d.runs[1].fills.len(), 12);
    }

    #[test]
    fn culling_and_lod_rebuilds() {
        let mut c = SceneCache::default();
        let circle = Shape::Circle(dotloom_geometry::Circle::new(Point::new(0.0, 0.0), 50.0).unwrap());
        let it = SceneItem {
            id: 1,
            layer: 0,
            bbox: circle.bbox(),
            flags: 0,
            prims: vec![Primitive::Shape {
                shape: circle,
                stroke: Some(Stroke { color: 0, width: 1.0, dash: vec![] }),
                fill: None,
            }],
        };
        c.apply(delta(vec![it, line_item(2, 1.0e6, 0)], Some(vec![1, 2])));
        let mut ts = TextSystem::with_default_font().unwrap();
        let mut st = PrepareStats::default();
        let view = Aabb::from_corners(Point::new(-100.0, -100.0), Point::new(100.0, 100.0));
        let vis = c.prepare(
            &FrameParams { visible: view, margin: 0.0, lod: 0, lod_tol: 0.25, theme: &Theme::light() },
            &mut ts,
            &mut st,
        );
        assert_eq!(vis.len(), 1, "far chunk culled");
        assert_eq!(st.tessellated_items, 1);
        let n0 = c.chunks[vis[0]].data.as_ref().unwrap().lines.len();
        // Same LOD: nothing rebuilt.
        let mut st2 = PrepareStats::default();
        c.prepare(
            &FrameParams { visible: view, margin: 0.0, lod: 0, lod_tol: 0.25, theme: &Theme::light() },
            &mut ts,
            &mut st2,
        );
        assert_eq!(st2.rebuilt_chunks, 0);
        // Zoom in 8×: curve re-tessellated with more segments.
        let mut st3 = PrepareStats::default();
        c.prepare(
            &FrameParams { visible: view, margin: 0.0, lod: 3, lod_tol: 0.25 / 8.0, theme: &Theme::light() },
            &mut ts,
            &mut st3,
        );
        assert_eq!(st3.rebuilt_chunks, 1);
        assert!(c.chunks[vis[0]].data.as_ref().unwrap().lines.len() > n0 * 2);
    }

    #[test]
    fn removal_and_reset() {
        let mut c = SceneCache::default();
        c.apply(delta(vec![line_item(1, 0.0, 0), line_item(2, 0.0, 0)], Some(vec![1, 2])));
        c.apply(SceneDelta { revision: 2, removals: vec![1], order: Some(vec![2]), ..SceneDelta::default() });
        assert_eq!(c.draw_order(), vec![2]);
        assert_eq!(c.revision(), 2);
        c.apply(SceneDelta { revision: 3, reset: true, ..SceneDelta::default() });
        assert!(c.is_empty());
        assert!(c.draw_order().is_empty());
    }
}

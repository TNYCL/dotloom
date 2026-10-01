//! Spatial queries: hit testing, area selection and snapping.
//!
//! All queries use the spatial index to collect nearby candidates first, so a
//! pointer move never scans every entity. Radii are in model units; hosts convert
//! screen tolerances with the current zoom (`ScreenTolerance::to_model`).

use std::collections::BTreeSet;

use dotloom_document::EntityId;
use dotloom_geometry::{Aabb, AnchorKind, Curve, FlattenTolerance, ModelTolerance, Point, intersect::intersect};
use serde::{Deserialize, Serialize};

use crate::{Engine, eval::Ctx};

/// A hit.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Hit {
    /// Entity.
    pub entity: EntityId,
    /// Distance from the query point to the outline (0 inside filled regions).
    pub distance: f64,
}

/// Area selection mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SelectMode {
    /// Entities fully inside the rectangle.
    #[default]
    Window,
    /// Entities touching the rectangle.
    Crossing,
}

/// Snap kinds in priority order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SnapKind {
    /// Endpoint / vertex / corner.
    Endpoint,
    /// Curve–curve intersection.
    Intersection,
    /// Center / centroid.
    Center,
    /// Midpoint.
    Midpoint,
    /// Circle quadrant.
    Quadrant,
    /// Other named anchor (insert points, plugin anchors).
    Anchor,
    /// Closest point on a curve.
    Nearest,
    /// Grid point.
    Grid,
}

/// Snap options.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SnapOptions {
    /// Endpoints, vertices, corners.
    pub endpoint: bool,
    /// Midpoints.
    pub midpoint: bool,
    /// Centers.
    pub center: bool,
    /// Quadrants.
    pub quadrant: bool,
    /// Intersections.
    pub intersection: bool,
    /// Other anchors.
    pub anchor: bool,
    /// Nearest point on curves.
    pub nearest: bool,
    /// Grid.
    pub grid: bool,
    /// Grid spacing override (mm).
    pub grid_spacing: Option<f64>,
}

impl Default for SnapOptions {
    fn default() -> Self {
        Self {
            endpoint: true,
            midpoint: true,
            center: true,
            quadrant: true,
            intersection: true,
            anchor: true,
            nearest: true,
            grid: false,
            grid_spacing: None,
        }
    }
}

/// A snap result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snap {
    /// Snapped point.
    pub point: Point,
    /// Kind.
    pub kind: SnapKind,
    /// Entity providing the snap.
    pub entity: Option<EntityId>,
    /// Anchor name (anchor snaps).
    pub anchor: Option<String>,
    /// Second entity (intersections).
    pub other: Option<EntityId>,
}

/// A snap query.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapQuery {
    /// Pointer position (model).
    pub point: Point,
    /// Snap radius (model units).
    pub radius: f64,
    /// Options.
    #[serde(default)]
    pub options: SnapOptions,
    /// Entities to ignore (e.g. the one being drawn or dragged).
    #[serde(default)]
    pub exclude: Vec<EntityId>,
    /// Previous result for hysteresis.
    #[serde(default)]
    pub previous: Option<Snap>,
}

fn anchor_kind(k: AnchorKind) -> SnapKind {
    match k {
        AnchorKind::Endpoint | AnchorKind::Vertex | AnchorKind::Corner | AnchorKind::Node => SnapKind::Endpoint,
        AnchorKind::Midpoint => SnapKind::Midpoint,
        AnchorKind::Center | AnchorKind::Centroid => SnapKind::Center,
        AnchorKind::Quadrant => SnapKind::Quadrant,
        AnchorKind::Insert => SnapKind::Anchor,
    }
}

impl Engine {
    fn visible_candidates(&self, ids: Vec<EntityId>, exclude: &BTreeSet<EntityId>) -> Vec<EntityId> {
        ids.into_iter()
            .filter(|id| !exclude.contains(id))
            .filter(|id| {
                self.doc.entity(*id).is_some_and(|e| !e.hidden && self.doc.layer(e.layer).is_some_and(|l| l.visible))
            })
            .collect()
    }

    /// Entities under `point` within `radius`, nearest first (ties: topmost first).
    pub fn hit_test(&mut self, point: Point, radius: f64) -> Vec<Hit> {
        if !point.is_finite() || (radius.is_nan() || radius < 0.0) {
            return Vec::new();
        }
        let ids = self.visible_candidates(self.index.query_point(point, radius), &BTreeSet::new());
        let ctx = Ctx { view: &self.doc, registry: &self.registry };
        let mut hits: Vec<(Hit, usize)> = Vec::new();
        for id in ids {
            let ev = self.cache.get(ctx, id);
            let mut best = f64::INFINITY;
            for s in ev.shapes() {
                if s.is_region()
                    && s.contains_point(point, FlattenTolerance(radius.max(1e-9) * 0.25))
                    && has_fill(&self.doc, id)
                {
                    best = 0.0;
                } else {
                    best = best.min(s.distance_to(point));
                }
            }
            if best <= radius {
                let z = self.doc.order_index(id).unwrap_or(0);
                hits.push((Hit { entity: id, distance: best }, z));
            }
        }
        hits.sort_by(|a, b| a.0.distance.total_cmp(&b.0.distance).then(b.1.cmp(&a.1)));
        hits.into_iter().map(|(h, _)| h).collect()
    }

    /// Area selection.
    pub fn select_in_rect(&mut self, rect: Aabb, mode: SelectMode) -> Vec<EntityId> {
        let ids = self.visible_candidates(self.index.query_rect(rect), &BTreeSet::new());
        let ctx = Ctx { view: &self.doc, registry: &self.registry };
        let mut out = Vec::new();
        for id in ids {
            let ev = self.cache.get(ctx, id);
            let ok = match mode {
                SelectMode::Window => rect.contains(ev.bbox),
                SelectMode::Crossing => {
                    rect.contains(ev.bbox) || ev.shapes().any(|s| s.intersects_rect(rect, ModelTolerance::DEFAULT))
                }
            };
            if ok {
                out.push(id);
            }
        }
        out
    }

    /// Snap a point. The result is a proposal: committing it still goes through the
    /// solver and independent checks, so a snap can never commit a hard-rule violation.
    pub fn snap(&mut self, q: &SnapQuery) -> Option<Snap> {
        if !q.point.is_finite() || (q.radius.is_nan() || q.radius <= 0.0) {
            return None;
        }
        let exclude: BTreeSet<EntityId> = q.exclude.iter().copied().collect();
        let ids = self.visible_candidates(self.index.query_point(q.point, q.radius), &exclude);
        let ctx = Ctx { view: &self.doc, registry: &self.registry };
        let o = q.options;
        let mut cands: Vec<Snap> = Vec::new();
        let mut curves: Vec<(EntityId, Curve)> = Vec::new();
        for id in ids.iter().take(64) {
            let ev = self.cache.get(ctx, *id);
            for a in &ev.anchors {
                let kind = anchor_kind(a.kind);
                let on = match kind {
                    SnapKind::Endpoint => o.endpoint,
                    SnapKind::Midpoint => o.midpoint,
                    SnapKind::Center => o.center,
                    SnapKind::Quadrant => o.quadrant,
                    SnapKind::Anchor => o.anchor,
                    _ => false,
                };
                if on && a.point.distance(q.point) <= q.radius {
                    cands.push(Snap {
                        point: a.point,
                        kind,
                        entity: Some(*id),
                        anchor: Some(a.name.clone()),
                        other: None,
                    });
                }
            }
            for s in ev.shapes() {
                for c in s.curves() {
                    if o.nearest {
                        let (_, p) = c.closest(q.point);
                        if p.distance(q.point) <= q.radius {
                            cands.push(Snap {
                                point: p,
                                kind: SnapKind::Nearest,
                                entity: Some(*id),
                                anchor: None,
                                other: None,
                            });
                        }
                    }
                    if curves.len() < 64 {
                        curves.push((*id, c));
                    }
                }
            }
        }
        if o.intersection {
            for i in 0..curves.len() {
                for j in i + 1..curves.len() {
                    let (Some((ea, ca)), Some((eb, cb))) = (curves.get(i), curves.get(j)) else { continue };
                    if ea == eb {
                        continue;
                    }
                    for p in intersect(ca, cb, ModelTolerance::DEFAULT).points {
                        if p.point.distance(q.point) <= q.radius {
                            cands.push(Snap {
                                point: p.point,
                                kind: SnapKind::Intersection,
                                entity: Some(*ea),
                                anchor: None,
                                other: Some(*eb),
                            });
                        }
                    }
                }
            }
        }
        if o.grid {
            let g = o.grid_spacing.unwrap_or(self.doc.settings.grid.spacing);
            if g > 0.0 && g.is_finite() {
                let p = Point::new((q.point.x / g).round() * g, (q.point.y / g).round() * g);
                if p.distance(q.point) <= q.radius {
                    cands.push(Snap { point: p, kind: SnapKind::Grid, entity: None, anchor: None, other: None });
                }
            }
        }
        let best = cands
            .into_iter()
            .min_by(|a, b| a.kind.cmp(&b.kind).then(a.point.distance(q.point).total_cmp(&b.point.distance(q.point))));
        // Hysteresis: keep the previous snap while it stays close and nothing better appears.
        if let Some(prev) = &q.previous
            && prev.point.distance(q.point) <= q.radius * 1.5
            && best.as_ref().is_none_or(|b| b.kind >= prev.kind)
            && prev.entity.is_none_or(|e| self.doc.entity(e).is_some() && !exclude.contains(&e))
            && self.snap_still_valid(prev)
        {
            return Some(prev.clone());
        }
        best
    }

    fn snap_still_valid(&mut self, s: &Snap) -> bool {
        match (s.entity, &s.anchor) {
            (Some(e), Some(a)) => {
                let ctx = Ctx { view: &self.doc, registry: &self.registry };
                self.cache
                    .get(ctx, e)
                    .anchor(a)
                    .is_some_and(|p| p.distance(s.point) <= 1e-9 * (1.0 + p.to_vector().length()))
            }
            _ => true,
        }
    }
}

fn has_fill(doc: &dotloom_document::Document, id: EntityId) -> bool {
    doc.entity(id).is_some_and(|e| e.style.fill.is_some() || !e.type_id.namespace().eq("dotloom"))
}

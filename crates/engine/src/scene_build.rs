//! Scene output: incremental [`SceneDelta`]s for renderers.
//!
//! Colors equal to `0` mean "theme default" (foreground for strokes and text);
//! renderers substitute their theme color, so documents stay theme-independent.

use std::collections::{BTreeMap, BTreeSet};

use dotloom_document::{Color, Entity, EntityId};
use dotloom_scene::{Primitive, SceneDelta, SceneItem, Stroke, flags};

use crate::{
    Engine,
    eval::{Drawable, Evaluated},
    registry::PrimStyle,
};

/// Theme-default color marker.
pub const THEME_DEFAULT: u32 = 0;

/// Pending scene changes.
#[derive(Debug, Clone, Default)]
pub struct SceneState {
    /// Entities to (re)emit.
    pub dirty: BTreeSet<EntityId>,
    /// Entities to remove.
    pub removed: BTreeSet<EntityId>,
    /// Emit everything.
    pub reset: bool,
    /// Draw order changed.
    pub order_changed: bool,
    /// Entities currently shown with preview content.
    pub preview: BTreeSet<EntityId>,
}

fn rgba(c: Option<Color>) -> Option<u32> {
    c.map(|c| c.0)
}

/// Convert evaluated drawables of an entity into scene primitives.
pub(crate) fn primitives(e: &Entity, layer_color: Option<Color>, ev: &Evaluated) -> Vec<Primitive> {
    let entity_stroke = rgba(e.style.stroke).or(rgba(layer_color));
    let width = e.style.stroke_width.unwrap_or(1.0) as f32;
    let dash: Vec<f32> = e.style.dash.as_ref().map(|d| d.iter().map(|x| *x as f32).collect()).unwrap_or_default();
    let stroke_for = |st: &PrimStyle| -> Option<Stroke> {
        if st.no_stroke {
            return None;
        }
        Some(Stroke {
            color: rgba(st.stroke).or(entity_stroke).unwrap_or(THEME_DEFAULT),
            width: st.width.map_or(width, |w| w as f32),
            dash: st.dash.as_ref().map_or_else(|| dash.clone(), |d| d.iter().map(|x| *x as f32).collect()),
        })
    };
    ev.drawables
        .iter()
        .map(|d| match d {
            Drawable::Shape(s, st) => {
                let fill = if s.is_region() { rgba(st.fill).or(rgba(e.style.fill)) } else { None };
                Primitive::Shape { shape: s.clone(), stroke: stroke_for(st), fill }
            }
            Drawable::Text(t, st) => {
                Primitive::Text { text: t.clone(), color: rgba(st.stroke).or(entity_stroke).unwrap_or(THEME_DEFAULT) }
            }
            Drawable::Arrow { tip, direction, size } => Primitive::Arrow {
                tip: *tip,
                direction: *direction,
                size: *size,
                color: entity_stroke.unwrap_or(THEME_DEFAULT),
            },
        })
        .collect()
}

impl Engine {
    fn layer_index(&self) -> BTreeMap<dotloom_document::LayerId, (u32, bool, bool, Option<Color>)> {
        self.doc
            .layers()
            .iter()
            .enumerate()
            .map(|(i, l)| (l.id, (u32::try_from(i).unwrap_or(u32::MAX), l.visible, l.locked, l.color)))
            .collect()
    }

    pub(crate) fn scene_item(
        &mut self,
        id: EntityId,
        preview: Option<&Evaluated>,
        preview_entity: Option<&Entity>,
    ) -> Option<SceneItem> {
        let layers = self.layer_index();
        let e = preview_entity.cloned().or_else(|| self.doc.entity(id).cloned())?;
        let (layer, visible, layer_locked, layer_color) = layers.get(&e.layer).copied()?;
        if e.hidden || !visible {
            return None;
        }
        let ev = match preview {
            Some(p) => p.clone(),
            None => {
                let ctx = crate::eval::Ctx { view: &self.doc, registry: &self.registry };
                self.cache.get(ctx, id).clone()
            }
        };
        let mut f = 0u8;
        if self.selection.contains(&id) {
            f |= flags::SELECTED;
        }
        if preview.is_some() {
            f |= flags::PREVIEW;
        }
        if e.locked || layer_locked {
            f |= flags::LOCKED;
        }
        if ev.read_only.is_some() {
            f |= flags::READONLY;
        }
        if ev.error.is_some() {
            f |= flags::PROBLEM;
        }
        Some(SceneItem { id: id.0, layer, bbox: ev.bbox, flags: f, prims: primitives(&e, layer_color, &ev) })
    }

    /// Changes since the last call (or the whole scene after load/reset).
    pub fn take_scene_delta(&mut self) -> SceneDelta {
        let reset = core::mem::take(&mut self.scene.reset);
        let mut delta = SceneDelta { revision: self.revision, reset, ..SceneDelta::default() };
        let ids: Vec<EntityId> = if reset {
            self.scene.dirty.clear();
            self.scene.removed.clear();
            self.scene.preview.clear();
            self.doc.order().to_vec()
        } else {
            let mut d: Vec<EntityId> = core::mem::take(&mut self.scene.dirty).into_iter().collect();
            d.extend(core::mem::take(&mut self.scene.preview));
            d.sort_unstable();
            d.dedup();
            d
        };
        for id in core::mem::take(&mut self.scene.removed) {
            if self.doc.entity(id).is_none() {
                delta.removals.push(id.0);
            }
        }
        for id in ids {
            match self.scene_item(id, None, None) {
                Some(item) => delta.upserts.push(item),
                None => delta.removals.push(id.0),
            }
        }
        let order_changed = core::mem::take(&mut self.scene.order_changed);
        if reset || order_changed {
            delta.order = Some(self.doc.order().iter().map(|id| id.0).collect());
        }
        delta.removals.sort_unstable();
        delta.removals.dedup();
        delta
    }

    /// The complete scene.
    pub fn full_scene(&mut self) -> SceneDelta {
        self.scene.reset = true;
        self.take_scene_delta()
    }
}

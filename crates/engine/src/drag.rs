//! Drag sessions: transient previews and a single history entry on release.
//!
//! * `begin_drag` captures the committed revision.
//! * `drag_to` solves the affected component towards the pointer (strong
//!   preferences, hard rules exact) and returns a *preview* scene delta. Nothing is
//!   committed. When a position cannot satisfy hard rules, the last valid preview
//!   stays on screen and the diagnostics are returned.
//! * `end_drag(true)` commits the last valid preview as one history entry;
//!   `end_drag(false)` (Escape, pointer cancel, focus loss) restores the committed
//!   scene.

use std::collections::BTreeSet;

use dotloom_constraints::{SolveJob, SolveOptions, Status};
use dotloom_document::EntityId;
use dotloom_geometry::{Affine, Point, TransformPolicy};
use dotloom_scene::SceneDelta;
use serde::{Deserialize, Serialize};

use crate::{
    Command, DocView, Engine, Overlay,
    command::{Applier, ApplyNotes},
    engine::CommitReport,
    error::{DiagnosticReport, EngineError},
    eval::{Ctx, evaluate},
    solve,
    view::OverlayData,
};

/// What is dragged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum DragSpec {
    /// Move entities by the pointer delta.
    Move {
        /// Entities.
        ids: Vec<EntityId>,
        /// Grab point (model).
        from: Point,
    },
    /// Drag one anchor (endpoint, corner, wall end, ...) to the pointer.
    Anchor {
        /// Entity.
        entity: EntityId,
        /// Anchor name.
        anchor: String,
    },
}

/// Result of a drag update.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DragPreview {
    /// Whether this position satisfies every hard rule.
    pub accepted: bool,
    /// Solver status.
    pub status: Status,
    /// Diagnostics when not accepted.
    pub diagnostics: Vec<DiagnosticReport>,
}

/// An active drag.
#[derive(Debug)]
pub(crate) struct DragSession {
    spec: DragSpec,
    base_revision: u64,
    last: Option<(OverlayData, ApplyNotes)>,
    shown: BTreeSet<EntityId>,
}

/// Solver options for interactive previews.
fn preview_options(base: SolveOptions) -> SolveOptions {
    SolveOptions { analyze: false, max_iterations: base.max_iterations.min(40), ..base }
}

impl Engine {
    /// Start dragging.
    pub fn begin_drag(&mut self, spec: DragSpec) -> Result<(), EngineError> {
        if self.pending.is_some() || self.drag.is_some() {
            return Err(EngineError::Busy { reason: "an interaction is in progress".into() });
        }
        let ids: Vec<EntityId> = match &spec {
            DragSpec::Move { ids, .. } => ids.clone(),
            DragSpec::Anchor { entity, anchor } => {
                let ctx = Ctx { view: &self.doc, registry: &self.registry };
                if evaluate(ctx, *entity).anchor(anchor).is_none() {
                    return Err(EngineError::Invalid { message: format!("{entity} has no anchor `{anchor}`") });
                }
                vec![*entity]
            }
        };
        if ids.is_empty() {
            return Err(EngineError::Invalid { message: "nothing to drag".into() });
        }
        for id in &ids {
            let e =
                self.doc.entity(*id).ok_or_else(|| EngineError::Invalid { message: format!("{id} does not exist") })?;
            if e.locked || self.doc.layer(e.layer).is_some_and(|l| l.locked) {
                return Err(EngineError::Command {
                    error: crate::CommandError::ReadOnly { what: id.to_string(), reason: "locked".into() },
                });
            }
        }
        self.drag = Some(DragSession { spec, base_revision: self.revision, last: None, shown: BTreeSet::new() });
        Ok(())
    }

    /// Whether a drag is active.
    #[must_use]
    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Update the drag to a pointer position. Returns the preview status and a
    /// preview scene delta (empty when the position is rejected).
    pub fn drag_to(&mut self, to: Point) -> Result<(DragPreview, SceneDelta), EngineError> {
        let Some(session) = self.drag.as_ref() else {
            return Err(EngineError::NotActive { what: "drag".into() });
        };
        if session.base_revision != self.revision || !to.is_finite() {
            return Err(EngineError::Stale { expected: session.base_revision, actual: self.revision });
        }
        let spec = session.spec.clone();
        let mut ov = Overlay::new(&self.doc);
        let mut notes = ApplyNotes::default();
        let anchor_target = match &spec {
            DragSpec::Move { ids, from } => {
                let mut ap = Applier {
                    ov: &mut ov,
                    registry: &self.registry,
                    reserved: &self.reserved,
                    notes: ApplyNotes::default(),
                };
                ap.apply(Command::Transform {
                    ids: ids.clone(),
                    transform: Affine::translate(to - *from),
                    policy: TransformPolicy::Strict,
                })?;
                notes = ap.notes;
                None
            }
            DragSpec::Anchor { entity, anchor } => {
                notes.touched.insert(*entity);
                Some((*entity, anchor.clone(), to))
            }
        };
        let opts = preview_options(self.options.solve);
        let target = anchor_target.as_ref().map(|(e, a, p)| (*e, a.as_str(), *p));
        // Pinned attempt first; relaxed attempt when it fails or misses the target.
        let mut chosen = None;
        for pin in [true, false] {
            let Some((problem, plan)) = solve::plan_with(&ov, &self.deps, &self.registry, &notes, true, target, pin)
            else {
                break;
            };
            let sol = SolveJob::new(problem.clone(), opts).into_solution();
            let good = sol.accepted() && solve::targets_met(&problem, &plan, &sol);
            if pin && !good {
                if sol.accepted() {
                    chosen = Some((sol, problem, plan));
                }
                continue;
            }
            if sol.accepted() || chosen.is_none() {
                chosen = Some((sol, problem, plan));
            }
            break;
        }
        let (accepted, status, diagnostics) = match chosen {
            None => (true, Status::Solved, Vec::new()),
            Some((sol, problem, plan)) => {
                match solve::finish(&mut ov, &self.registry, &problem, &plan, &sol, &mut notes) {
                    Ok(()) => (true, sol.status, Vec::new()),
                    Err(EngineError::Solve { failure }) => (false, failure.status, failure.diagnostics),
                    Err(EngineError::Validation { .. } | EngineError::Invalid { .. }) => {
                        (false, sol.status, Vec::new())
                    }
                    Err(other) => return Err(other),
                }
            }
        };
        let mut delta = SceneDelta { revision: self.revision, preview: true, ..SceneDelta::default() };
        if accepted {
            // Preview items for changed entities and everything that depends on them.
            let changed: Vec<EntityId> = ov.entities.keys().copied().collect();
            let affected = self.dependents_closure(&changed);
            let ctx = Ctx { view: &ov, registry: &self.registry };
            let evs: Vec<(EntityId, crate::eval::Evaluated, Option<dotloom_document::Entity>)> =
                affected.iter().map(|id| (*id, evaluate(ctx, *id), ov.entity(*id).cloned())).collect();
            let data = ov.into_data();
            for (id, ev, ent) in evs {
                if let Some(item) = self.scene_item(id, Some(&ev), ent.as_ref()) {
                    delta.upserts.push(item);
                }
            }
            if let Some(s) = self.drag.as_mut() {
                s.shown.extend(affected.iter().copied());
                s.last = Some((data, notes));
            }
        }
        Ok((DragPreview { accepted, status, diagnostics }, delta))
    }

    /// Finish the drag: commit the last valid preview as one history entry, or
    /// cancel and restore the committed scene.
    pub fn end_drag(&mut self, commit: bool) -> Result<Option<CommitReport>, EngineError> {
        let Some(session) = self.drag.take() else {
            return Err(EngineError::NotActive { what: "drag".into() });
        };
        self.scene.preview.extend(session.shown.iter().copied());
        if !commit || session.base_revision != self.revision {
            return Ok(None);
        }
        let Some((data, notes)) = session.last else { return Ok(None) };
        let label = match session.spec {
            DragSpec::Move { .. } => "Move",
            DragSpec::Anchor { .. } => "Drag point",
        };
        // Re-solve with full analysis from the preview state to report DOF, then commit.
        let id = self.start_pending(data, notes, label.into(), "drag", true)?;
        loop {
            match self.step_pending(u32::MAX)? {
                crate::engine::PendingState::Done(r) => return r.map(Some),
                crate::engine::PendingState::Running { id: pid } if pid == id => {}
                crate::engine::PendingState::Running { .. } => {
                    return Err(EngineError::Busy { reason: "unexpected pending request".into() });
                }
            }
        }
    }
}

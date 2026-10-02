//! The engine: the single editable owner of a document.

use std::collections::{BTreeMap, BTreeSet};

use dotloom_constraints::{Progress, SolveJob, SolveOptions, Status};
use dotloom_document::{ConstraintId, Document, EntityId, Limits, TypeId};
use dotloom_geometry::SpatialIndex;
use serde::{Deserialize, Serialize};

use crate::{
    Command, DocView, Overlay,
    command::{Applier, ApplyNotes},
    error::{EngineError, SolveFailure},
    eval::{Ctx, EvalCache},
    history::{Change, History},
    index::DepIndex,
    registry::{EntityTypeDef, Registry},
    scene_build::SceneState,
    solve::{self, Plan},
    view::OverlayData,
};

/// A group of commands applied atomically.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transaction {
    /// Label shown in history.
    #[serde(default)]
    pub label: String,
    /// Commands.
    pub commands: Vec<Command>,
}

impl Transaction {
    /// Transaction with a label.
    #[must_use]
    pub fn new(label: impl Into<String>, commands: Vec<Command>) -> Self {
        Self { label: label.into(), commands }
    }
}

/// Options of a single request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyOptions {
    /// Reject the request unless the document is at this revision.
    #[serde(default)]
    pub expected_revision: Option<u64>,
}

/// Successful commit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitReport {
    /// New revision.
    pub revision: u64,
    /// Solver status of the affected rules (`solved` when nothing was solved).
    pub status: Status,
    /// Non-fatal diagnostics (redundant rules, ...).
    pub diagnostics: Vec<crate::error::DiagnosticReport>,
    /// Created entities.
    pub created: Vec<EntityId>,
    /// Created constraints.
    pub created_constraints: Vec<ConstraintId>,
    /// Deleted entities.
    pub deleted: Vec<EntityId>,
    /// Constraints removed as a consequence.
    pub removed_constraints: Vec<ConstraintId>,
    /// Entities whose content changed.
    pub changed: Vec<EntityId>,
    /// Notes.
    pub notes: Vec<String>,
    /// Whether the change can be undone (false if it exceeded the history budget).
    pub undo_available: bool,
    /// Solver statistics, when rules were solved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solver: Option<SolverStats>,
}

/// What the solver did for a commit (diagnostics and performance work).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SolverStats {
    /// Iterations of the chosen attempt.
    pub iterations: u32,
    /// Solve attempts run (pinned, exact, relaxed).
    pub attempts: u32,
    /// Solver variables of the chosen attempt.
    pub variables: u32,
    /// Rules of the chosen attempt.
    pub rules: u32,
    /// Connected components solved.
    pub components: u32,
}

/// Engine event, emitted after state changes (never during a commit).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Event {
    /// A transaction, undo or redo was committed.
    Committed {
        /// New revision.
        revision: u64,
        /// History label.
        label: String,
        /// `apply` | `undo` | `redo` | `drag` | `load`.
        cause: String,
        /// Changed entities (created, updated, deleted).
        entities: Vec<EntityId>,
        /// Changed constraints.
        constraints: Vec<ConstraintId>,
    },
    /// Selection changed.
    SelectionChanged {
        /// Selected entities.
        selection: Vec<EntityId>,
    },
    /// Undo/redo availability changed.
    HistoryChanged {
        /// Undo possible.
        can_undo: bool,
        /// Redo possible.
        can_redo: bool,
        /// Next undo label.
        undo_label: Option<String>,
        /// Next redo label.
        redo_label: Option<String>,
    },
    /// Plugin types changed.
    PluginsChanged,
}

/// Engine configuration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EngineOptions {
    /// Solver options for commits.
    pub solve: SolveOptions,
    /// History entry limit.
    pub history_entries: usize,
    /// History byte budget.
    pub history_bytes: usize,
    /// Document limits.
    pub limits: Limits,
}

impl Default for EngineOptions {
    fn default() -> Self {
        Self {
            solve: SolveOptions::default(),
            history_entries: 500,
            history_bytes: 64 << 20,
            limits: Limits::default(),
        }
    }
}

/// A commit whose solve runs in budgeted steps.
#[derive(Debug)]
pub(crate) struct Pending {
    pub id: u64,
    pub base_revision: u64,
    pub data: OverlayData,
    pub notes: ApplyNotes,
    pub label: String,
    pub cause: &'static str,
    pub job: Option<(SolveJob, dotloom_constraints::Problem, Plan)>,
    /// Further attempts, tried in order when the current one is infeasible or
    /// misses a requested value.
    pub fallbacks: std::collections::VecDeque<(dotloom_constraints::Problem, Plan)>,
    /// First accepted result, used if every later attempt fails.
    pub first: Option<(dotloom_constraints::Solution, dotloom_constraints::Problem, Plan)>,
    /// The attempt chosen for committing.
    pub chosen: Option<(dotloom_constraints::Solution, dotloom_constraints::Problem, Plan)>,
    /// Attempts started so far.
    pub attempts: u32,
}

/// Progress of a pending commit.
#[derive(Debug, Clone, PartialEq)]
pub enum PendingState {
    /// More steps needed.
    Running {
        /// Request.
        id: u64,
    },
    /// Finished (boxed: the report is much larger than the running state).
    Done(Box<Result<CommitReport, EngineError>>),
}

/// The Dotloom engine.
#[derive(Debug)]
pub struct Engine {
    pub(crate) doc: Document,
    pub(crate) registry: Registry,
    pub(crate) history: History,
    pub(crate) revision: u64,
    pub(crate) selection: BTreeSet<EntityId>,
    pub(crate) reserved: BTreeSet<u64>,
    pub(crate) cache: EvalCache,
    pub(crate) index: SpatialIndex<EntityId>,
    pub(crate) deps: DepIndex,
    pub(crate) scene: SceneState,
    pub(crate) events: Vec<Event>,
    pub(crate) pending: Option<Pending>,
    pub(crate) drag: Option<crate::drag::DragSession>,
    pub(crate) options: EngineOptions,
    next_pending: u64,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new(EngineOptions::default())
    }
}

impl Engine {
    /// Engine with an empty document.
    #[must_use]
    pub fn new(options: EngineOptions) -> Self {
        let mut e = Self {
            doc: Document::new(),
            registry: Registry::new(),
            history: History::with_limits(options.history_entries, options.history_bytes),
            revision: 0,
            selection: BTreeSet::new(),
            reserved: BTreeSet::new(),
            cache: EvalCache::default(),
            index: SpatialIndex::new(),
            deps: DepIndex::default(),
            scene: SceneState::default(),
            events: Vec::new(),
            pending: None,
            drag: None,
            options,
            next_pending: 1,
        };
        e.rebuild_derived();
        e
    }

    /// The committed document (read-only).
    #[must_use]
    pub fn document(&self) -> &Document {
        &self.doc
    }

    /// Current revision (increments on every commit, undo and redo).
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Type registry.
    #[must_use]
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    pub(crate) fn ctx(&self) -> Ctx<'_> {
        Ctx { view: &self.doc, registry: &self.registry }
    }

    /// Replace the document. Entity type migrations of registered plugins are
    /// applied; the document is validated; history is cleared.
    pub fn load(&mut self, mut doc: Document) -> Result<u64, EngineError> {
        if self.pending.is_some() || self.drag.is_some() {
            return Err(EngineError::Busy { reason: "an interaction is in progress".into() });
        }
        crate::migrate::migrate_entities(&mut doc, &self.registry);
        if let Some(err) = doc.validate_all(&self.options.limits).into_iter().next() {
            return Err(EngineError::Load { message: err.to_string() });
        }
        self.doc = doc;
        self.history.clear();
        self.selection.clear();
        self.reserved.clear();
        self.revision += 1;
        self.rebuild_derived();
        self.scene.reset = true;
        self.events.push(Event::Committed {
            revision: self.revision,
            label: "load".into(),
            cause: "load".into(),
            entities: Vec::new(),
            constraints: Vec::new(),
        });
        self.emit_history();
        self.events.push(Event::SelectionChanged { selection: Vec::new() });
        Ok(self.revision)
    }

    fn rebuild_derived(&mut self) {
        self.cache.clear();
        self.deps = DepIndex::build(&self.doc);
        let ctx = Ctx { view: &self.doc, registry: &self.registry };
        let mut items = Vec::with_capacity(self.doc.entity_count());
        for e in self.doc.entities() {
            let bb = self.cache.get(ctx, e.id).bbox;
            items.push((e.id, bb));
        }
        self.index = SpatialIndex::bulk_load(items);
        self.scene.reset = true;
    }

    // --- plugins --------------------------------------------------------------

    /// Register a plugin entity type.
    pub fn register_type(&mut self, def: EntityTypeDef, plugin: &str) -> Result<(), EngineError> {
        let t = def.type_id.clone();
        self.registry.register(def, plugin).map_err(|e| EngineError::Plugin { message: e.to_string() })?;
        crate::migrate::migrate_entities(&mut self.doc, &self.registry);
        self.refresh_type(&t);
        self.events.push(Event::PluginsChanged);
        Ok(())
    }

    /// Remove a plugin entity type (its entities become read-only, data is kept).
    pub fn unregister_type(&mut self, t: &TypeId) -> Result<(), EngineError> {
        self.registry.unregister(t).map_err(|e| EngineError::Plugin { message: e.to_string() })?;
        self.refresh_type(t);
        self.events.push(Event::PluginsChanged);
        Ok(())
    }

    /// Enable or disable a plugin entity type.
    pub fn set_type_enabled(&mut self, t: &TypeId, enabled: bool) -> Result<(), EngineError> {
        self.registry.set_enabled(t, enabled).map_err(|e| EngineError::Plugin { message: e.to_string() })?;
        self.refresh_type(t);
        self.events.push(Event::PluginsChanged);
        Ok(())
    }

    fn refresh_type(&mut self, t: &TypeId) {
        let ids: Vec<EntityId> = self.doc.entities().filter(|e| &e.type_id == t).map(|e| e.id).collect();
        let affected = self.dependents_closure(&ids);
        self.refresh_entities(&affected);
    }

    /// Entities whose evaluation depends on `ids` (transitively through references).
    pub(crate) fn dependents_closure(&self, ids: &[EntityId]) -> BTreeSet<EntityId> {
        let mut out: BTreeSet<EntityId> = BTreeSet::new();
        let mut q: Vec<EntityId> = ids.to_vec();
        while let Some(id) = q.pop() {
            if out.insert(id) {
                q.extend(self.deps.referrers_of(id));
            }
        }
        out
    }

    /// Re-evaluate entities, update the spatial index and mark them for the scene.
    pub(crate) fn refresh_entities(&mut self, ids: &BTreeSet<EntityId>) {
        self.cache.invalidate(ids.iter().copied());
        let ctx = Ctx { view: &self.doc, registry: &self.registry };
        for id in ids {
            if self.doc.entity(*id).is_some() {
                let bb = self.cache.get(ctx, *id).bbox;
                self.index.upsert(*id, bb);
                self.scene.dirty.insert(*id);
            } else {
                self.index.remove(*id);
                self.scene.removed.insert(*id);
            }
        }
    }

    // --- ids ------------------------------------------------------------------

    /// Reserve fresh IDs for use in the next transactions (e.g. a tool that
    /// creates a line and a constraint on it in one transaction).
    pub fn reserve_ids(&mut self, n: usize) -> Vec<u64> {
        let n = n.min(10_000);
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            let id = self.doc.alloc_id();
            self.reserved.insert(id);
            out.push(id);
        }
        out
    }

    // --- transactions ---------------------------------------------------------

    fn check_revision(&self, opts: ApplyOptions) -> Result<(), EngineError> {
        match opts.expected_revision {
            Some(r) if r != self.revision => Err(EngineError::Stale { expected: r, actual: self.revision }),
            _ => Ok(()),
        }
    }

    /// Apply a transaction synchronously.
    pub fn apply(&mut self, tx: Transaction, opts: ApplyOptions) -> Result<CommitReport, EngineError> {
        let id = self.begin_apply(tx, opts)?;
        loop {
            match self.step_pending(u32::MAX)? {
                PendingState::Done(r) => return *r,
                PendingState::Running { id: pid } if pid == id => {}
                PendingState::Running { .. } => {
                    return Err(EngineError::Busy { reason: "unexpected pending request".into() });
                }
            }
        }
    }

    /// Start a transaction whose solve runs in budgeted steps
    /// ([`Engine::step_pending`]). Returns the pending request ID.
    pub fn begin_apply(&mut self, tx: Transaction, opts: ApplyOptions) -> Result<u64, EngineError> {
        if self.pending.is_some() {
            return Err(EngineError::Busy { reason: "another transaction is being solved".into() });
        }
        if self.drag.is_some() {
            return Err(EngineError::Busy { reason: "a drag is in progress".into() });
        }
        self.check_revision(opts)?;
        let (data, notes) = self.apply_commands(tx.commands)?;
        self.start_pending(data, notes, if tx.label.is_empty() { "edit".into() } else { tx.label }, "apply", false)
    }

    pub(crate) fn apply_commands(&self, commands: Vec<Command>) -> Result<(OverlayData, ApplyNotes), EngineError> {
        if commands.len() > 100_000 {
            return Err(EngineError::Invalid { message: "too many commands in one transaction".into() });
        }
        let mut ov = Overlay::new(&self.doc);
        let notes = {
            let mut ap = Applier {
                ov: &mut ov,
                registry: &self.registry,
                reserved: &self.reserved,
                notes: ApplyNotes::default(),
            };
            for c in commands {
                ap.apply(c)?;
            }
            ap.notes
        };
        crate::validate::validate_overlay(&ov, &self.registry, &notes, &self.options.limits)?;
        Ok((ov.into_data(), notes))
    }

    pub(crate) fn start_pending(
        &mut self,
        data: OverlayData,
        notes: ApplyNotes,
        label: String,
        cause: &'static str,
        prefer: bool,
    ) -> Result<u64, EngineError> {
        let ov = Overlay::from_data(&self.doc, data);
        // Attempts (see ADR-0004 §11): requested values exact with the rest pinned,
        // then exact with everything free (by stay priority), then — only when some
        // values are preferences — the relaxed nearest-feasible solve.
        let soft = prefer || notes.edits.iter().any(|e| !e.3);
        let attempts: &[(bool, bool)] =
            if soft { &[(true, true), (false, true), (false, false)] } else { &[(true, false), (false, false)] };
        let mut plans: std::collections::VecDeque<_> = attempts
            .iter()
            .filter_map(|&(pin, hard)| {
                let a = solve::Attempt { target: None, pin, hard_target: hard };
                solve::plan_with(&ov, &self.deps, &self.registry, &notes, prefer, a)
            })
            .collect();
        let job = plans.pop_front().map(|(problem, plan)| {
            let job = SolveJob::new(problem.clone(), self.options.solve);
            (job, problem, plan)
        });
        let id = self.next_pending;
        self.next_pending += 1;
        self.pending = Some(Pending {
            id,
            base_revision: self.revision,
            data: ov.into_data(),
            notes,
            label,
            cause,
            attempts: u32::from(job.is_some()),
            job,
            fallbacks: plans,
            first: None,
            chosen: None,
        });
        Ok(id)
    }

    /// Whether a pending commit exists.
    #[must_use]
    pub fn has_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Run the pending commit for at most `budget` solver iterations.
    pub fn step_pending(&mut self, budget: u32) -> Result<PendingState, EngineError> {
        let Some(p) = self.pending.as_mut() else {
            return Err(EngineError::NotActive { what: "pending transaction".into() });
        };
        let id = p.id;
        if p.base_revision != self.revision {
            let expected = p.base_revision;
            self.pending = None;
            return Ok(PendingState::Done(Box::new(Err(EngineError::Stale { expected, actual: self.revision }))));
        }
        if let Some((job, _, _)) = p.job.as_mut()
            && job.step(budget) != Progress::Finished
        {
            return Ok(PendingState::Running { id });
        }
        // The current attempt finished: keep it if it is acceptable and meets every
        // requested value; otherwise start the next attempt. The last attempt's
        // result is used unless it failed and an earlier one was acceptable.
        if let Some((job, problem, plan)) = p.job.take() {
            let sol = job.into_solution();
            let good = sol.accepted() && solve::targets_met(&problem, &plan, &sol);
            match p.fallbacks.pop_front() {
                Some((next, next_plan)) if !good => {
                    if sol.accepted() && p.first.is_none() {
                        p.first = Some((sol, problem, plan));
                    }
                    p.job = Some((SolveJob::new(next.clone(), self.options.solve), next, next_plan));
                    p.attempts += 1;
                    return Ok(PendingState::Running { id });
                }
                _ => {
                    p.chosen = match p.first.take() {
                        Some(f) if !sol.accepted() => Some(f),
                        _ => Some((sol, problem, plan)),
                    };
                }
            }
        }
        let Some(p) = self.pending.take() else {
            return Err(EngineError::NotActive { what: "pending transaction".into() });
        };
        Ok(PendingState::Done(Box::new(self.finish_pending(p))))
    }

    /// Cancel the pending commit (the document is unchanged).
    pub fn cancel_pending(&mut self) -> bool {
        self.pending.take().is_some()
    }

    fn finish_pending(&mut self, p: Pending) -> Result<CommitReport, EngineError> {
        let Pending { data, mut notes, label, cause, chosen, attempts, .. } = p;
        let mut ov = Overlay::from_data(&self.doc, data);
        let mut status = Status::Solved;
        let mut diagnostics = Vec::new();
        let solver = chosen.as_ref().map(|(sol, problem, _)| SolverStats {
            iterations: sol.iterations,
            attempts,
            variables: u32::try_from(problem.vars.len()).unwrap_or(u32::MAX),
            rules: u32::try_from(problem.rules.len()).unwrap_or(u32::MAX),
            components: u32::try_from(sol.components.len()).unwrap_or(u32::MAX),
        });
        if let Some((sol, problem, plan)) = chosen {
            status = sol.status;
            if let Err(err) = solve::finish(&mut ov, &self.registry, &problem, &plan, &sol, &mut notes) {
                let err = match err {
                    EngineError::Solve { mut failure } => {
                        failure.nearest = solve::nearest(&ov, &self.deps, &self.registry, &notes, &self.options.solve);
                        EngineError::Solve { failure: SolveFailure { ..failure } }
                    }
                    other => other,
                };
                return Err(err);
            }
            diagnostics = solve::reports(&problem, &plan, &sol);
        }
        let data = ov.into_data();
        let mut report = self.commit(data, notes, label, cause, status, diagnostics);
        report.solver = solver;
        Ok(report)
    }

    fn commit(
        &mut self,
        data: OverlayData,
        notes: ApplyNotes,
        label: String,
        cause: &str,
        status: Status,
        diagnostics: Vec<crate::error::DiagnosticReport>,
    ) -> CommitReport {
        let ov = Overlay::from_data(&self.doc, data);
        let change = crate::apply::change_of(&ov);
        let next_id = ov.next_id;
        drop(ov);
        change.apply(&mut self.doc, true);
        self.doc.reserve_id(next_id.saturating_sub(1));
        for id in &notes.created {
            self.reserved.remove(&id.0);
        }
        for id in &notes.created_constraints {
            self.reserved.remove(&id.0);
        }
        self.revision += 1;
        let changed_entities: Vec<EntityId> = change.entity_ids().collect();
        let changed_constraints: Vec<ConstraintId> = change.constraints.iter().map(|c| c.0).collect();
        self.update_after_change(&change);
        let undo_available =
            if change.is_empty() { self.history.can_undo() } else { self.history.push(label.clone(), change) };
        self.events.push(Event::Committed {
            revision: self.revision,
            label,
            cause: cause.into(),
            entities: changed_entities.clone(),
            constraints: changed_constraints,
        });
        self.emit_history();
        CommitReport {
            revision: self.revision,
            status,
            diagnostics,
            created: notes.created,
            created_constraints: notes.created_constraints,
            deleted: notes.deleted,
            removed_constraints: notes.removed_constraints,
            changed: changed_entities,
            notes: notes.notes,
            undo_available,
            solver: None,
        }
    }

    /// Update indexes, caches, scene and selection after a change was applied.
    pub(crate) fn update_after_change(&mut self, change: &Change) {
        for (_, before, after) in &change.entities {
            if let Some((e, _)) = before {
                self.deps.remove_entity(e);
            }
            if let Some((e, _)) = after {
                self.deps.add_entity(e);
            }
        }
        for (_, before, after) in &change.constraints {
            if let Some(c) = before {
                self.deps.remove_constraint(c);
            }
            if let Some(c) = after {
                self.deps.add_constraint(c);
            }
        }
        let ids: Vec<EntityId> = change.entity_ids().collect();
        // Before-states may have been referenced by entities that no longer reference them.
        let mut affected = self.dependents_closure(&ids);
        for (_, b, _) in &change.entities {
            if let Some((e, _)) = b {
                affected.extend(e.referenced_entities());
            }
        }
        if change.layers.is_some() || change.settings.is_some() {
            affected.extend(self.doc.order().iter().copied());
        }
        self.refresh_entities(&affected);
        if change.entities.iter().any(|(_, b, a)| b.as_ref().map(|x| x.1) != a.as_ref().map(|x| x.1)) {
            self.scene.order_changed = true;
        }
        let before = self.selection.len();
        self.selection.retain(|id| self.doc.entity(*id).is_some());
        if self.selection.len() != before {
            self.events.push(Event::SelectionChanged { selection: self.selection.iter().copied().collect() });
        }
    }

    pub(crate) fn emit_history(&mut self) {
        self.events.push(Event::HistoryChanged {
            can_undo: self.history.can_undo(),
            can_redo: self.history.can_redo(),
            undo_label: self.history.undo_label().map(ToOwned::to_owned),
            redo_label: self.history.redo_label().map(ToOwned::to_owned),
        });
    }

    /// Undo the last commit (applies the stored before-state; no solving).
    pub fn undo(&mut self, opts: ApplyOptions) -> Result<u64, EngineError> {
        self.check_idle()?;
        self.check_revision(opts)?;
        let entry = self.history.take_undo().ok_or(EngineError::NothingToUndo)?;
        entry.change.apply(&mut self.doc, false);
        self.revision += 1;
        self.update_after_change(&entry.change);
        self.events.push(Event::Committed {
            revision: self.revision,
            label: entry.label.clone(),
            cause: "undo".into(),
            entities: entry.change.entity_ids().collect(),
            constraints: entry.change.constraints.iter().map(|c| c.0).collect(),
        });
        self.history.push_redo(entry);
        self.emit_history();
        Ok(self.revision)
    }

    /// Redo the last undone commit.
    pub fn redo(&mut self, opts: ApplyOptions) -> Result<u64, EngineError> {
        self.check_idle()?;
        self.check_revision(opts)?;
        let entry = self.history.take_redo().ok_or(EngineError::NothingToRedo)?;
        entry.change.apply(&mut self.doc, true);
        self.revision += 1;
        self.update_after_change(&entry.change);
        self.events.push(Event::Committed {
            revision: self.revision,
            label: entry.label.clone(),
            cause: "redo".into(),
            entities: entry.change.entity_ids().collect(),
            constraints: entry.change.constraints.iter().map(|c| c.0).collect(),
        });
        self.history.push_undo_keep_redo(entry);
        self.emit_history();
        Ok(self.revision)
    }

    fn check_idle(&self) -> Result<(), EngineError> {
        if self.pending.is_some() || self.drag.is_some() {
            return Err(EngineError::Busy { reason: "an interaction is in progress".into() });
        }
        Ok(())
    }

    /// Whether undo is possible.
    #[must_use]
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    /// Whether redo is possible.
    #[must_use]
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// History memory estimate in bytes.
    #[must_use]
    pub fn history_bytes(&self) -> usize {
        self.history.bytes()
    }

    /// Drain pending events.
    pub fn take_events(&mut self) -> Vec<Event> {
        core::mem::take(&mut self.events)
    }

    // --- selection ------------------------------------------------------------

    /// Current selection.
    #[must_use]
    pub fn selection(&self) -> Vec<EntityId> {
        self.selection.iter().copied().collect()
    }

    /// Replace the selection (unknown IDs are ignored).
    pub fn set_selection(&mut self, ids: &[EntityId]) {
        let new: BTreeSet<EntityId> = ids.iter().copied().filter(|id| self.doc.entity(*id).is_some()).collect();
        if new == self.selection {
            return;
        }
        let changed: BTreeSet<EntityId> = new.symmetric_difference(&self.selection).copied().collect();
        self.selection = new;
        self.scene.dirty.extend(changed);
        self.events.push(Event::SelectionChanged { selection: self.selection.iter().copied().collect() });
    }

    /// Evaluation of an entity (anchors, drawables) from the committed document.
    pub fn evaluate(&mut self, id: EntityId) -> Option<crate::eval::Evaluated> {
        self.doc.entity(id)?;
        let ctx = Ctx { view: &self.doc, registry: &self.registry };
        Some(self.cache.get(ctx, id).clone())
    }

    /// Constraints that reference an entity.
    #[must_use]
    pub fn constraints_of(&self, id: EntityId) -> Vec<ConstraintId> {
        self.deps.constraints_of(id).collect()
    }

    /// Analyse every constraint of the document (status, DOF, conflicts) without
    /// changing anything.
    #[must_use]
    pub fn analyze(&self) -> (Status, Vec<crate::error::DiagnosticReport>) {
        let ov = Overlay::new(&self.doc);
        let mut notes = ApplyNotes::default();
        for c in self.doc.constraints() {
            notes.touched_constraints.insert(c.id);
        }
        for e in self.doc.entities() {
            if !e.type_id.namespace().eq("dotloom") {
                notes.touched.insert(e.id);
            }
        }
        match solve::plan(&ov, &self.deps, &self.registry, &notes, false, false) {
            None => (Status::Solved, Vec::new()),
            Some((problem, plan)) => {
                let sol = dotloom_constraints::solve(&problem, &self.options.solve);
                (sol.status, solve::reports(&problem, &plan, &sol))
            }
        }
    }

    /// Check every enabled hard rule (document constraints and plugin templates)
    /// against the stored values with the independent evaluator, without solving.
    /// Returns violations as `(what, residual, tolerance)`.
    #[must_use]
    pub fn verify(&self) -> Vec<(String, f64, f64)> {
        let ctx = self.ctx();
        let ids: std::collections::BTreeSet<EntityId> = self.doc.order().iter().copied().collect();
        let scales = solve::scales_for(ctx, &ids);
        let mut checker = crate::check::Checker::new(ctx, scales);
        let mut out = Vec::new();
        for c in self.doc.constraints() {
            if !c.enabled || c.strength != dotloom_document::StrengthSpec::Required {
                continue;
            }
            match checker.constraint(c) {
                Ok((r, t)) if r.is_nan() || r > t => out.push((c.id.to_string(), r, t)),
                Ok(_) => {}
                Err(m) => out.push((format!("{}: {m}", c.id), f64::INFINITY, 0.0)),
            }
        }
        for id in &ids {
            for (label, r, t) in checker.templates(*id) {
                if r.is_nan() || r > t {
                    out.push((format!("{id} {label}"), r, t));
                }
            }
        }
        out
    }

    /// Parameter values of an entity `(name, value)`.
    #[must_use]
    pub fn params_of(&self, id: EntityId) -> BTreeMap<String, f64> {
        let ctx = self.ctx();
        let Some(e) = self.doc.entity(id) else { return BTreeMap::new() };
        let names: Vec<String> = if e.type_id.namespace() == "dotloom" {
            e.geometry
                .as_ref()
                .map(|g| dotloom_document::builtin::geometry_params(g).into_iter().map(|(n, _, _)| n).collect())
                .unwrap_or_default()
        } else {
            self.registry.get(&e.type_id).map(|t| t.compiled.def.props.keys().cloned().collect()).unwrap_or_default()
        };
        names.into_iter().filter_map(|n| crate::eval::param_value(ctx, e, &n).map(|v| (n, v))).collect()
    }
}

impl DocView for Engine {
    fn entity(&self, id: EntityId) -> Option<&dotloom_document::Entity> {
        self.doc.entity(id)
    }
    fn constraint(&self, id: ConstraintId) -> Option<&dotloom_document::Constraint> {
        self.doc.constraint(id)
    }
    fn layer(&self, id: dotloom_document::LayerId) -> Option<&dotloom_document::Layer> {
        self.doc.layer(id)
    }
    fn group(&self, id: dotloom_document::GroupId) -> Option<&dotloom_document::Group> {
        self.doc.group(id)
    }
    fn settings(&self) -> &dotloom_document::Settings {
        &self.doc.settings
    }
}

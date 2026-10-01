//! Structural validation of a transaction overlay before solving and commit.

use std::collections::{BTreeMap, BTreeSet};

use dotloom_document::{
    EntityId, GroupId, Limits, PropValue,
    builtin::{builtin_shape_kind, is_builtin},
};

use crate::{
    DocView, Overlay,
    command::ApplyNotes,
    error::EngineError,
    eval::{Ctx, evaluate, plugin_for},
    registry::{PropDef, Registry},
};

fn invalid(message: String) -> EngineError {
    EngineError::Invalid { message }
}

/// Check every object the overlay touched (and references to deleted objects).
pub(crate) fn validate_overlay(
    ov: &Overlay<'_>,
    registry: &Registry,
    notes: &ApplyNotes,
    limits: &Limits,
) -> Result<(), EngineError> {
    let added = ov.entities.iter().filter(|(id, e)| e.is_some() && ov.base.entity(**id).is_none()).count();
    let removed = ov.entities.iter().filter(|(id, e)| e.is_none() && ov.base.entity(**id).is_some()).count();
    if ov.base.entity_count() + added - removed.min(ov.base.entity_count() + added) > limits.max_entities {
        return Err(invalid(format!("more than {} entities", limits.max_entities)));
    }
    let ctx = Ctx { view: ov, registry };
    let deleted: BTreeSet<EntityId> = ov.entities.iter().filter(|(_, e)| e.is_none()).map(|(id, _)| *id).collect();
    for (id, e) in &ov.entities {
        let Some(e) = e else { continue };
        if ov.layer(e.layer).is_none() {
            return Err(invalid(format!("{id} is on missing layer {}", e.layer)));
        }
        if !e.transform.is_finite() || e.transform.inverse().is_err() {
            return Err(invalid(format!("{id} has a singular transform")));
        }
        if let Some(kind) = builtin_shape_kind(&e.type_id) {
            let g = e.geometry.as_ref().ok_or_else(|| invalid(format!("{id} has no geometry")))?;
            if g.kind() != kind {
                return Err(invalid(format!("{id}: geometry kind mismatch")));
            }
            g.validate().map_err(|x| invalid(format!("{id}: {x}")))?;
        }
        for (k, v) in &e.props {
            if !v.is_finite() {
                return Err(invalid(format!("{id}.{k} is not finite")));
            }
            match v {
                PropValue::Ref(r) if ov.entity(r.entity).is_none() => {
                    return Err(invalid(format!("{id}.{k} references missing {}", r.entity)));
                }
                PropValue::Anchor(a) => {
                    if ov.entity(a.entity).is_none() {
                        return Err(invalid(format!("{id}.{k} references missing {}", a.entity)));
                    }
                    if evaluate(ctx, a.entity).anchor(&a.anchor).is_none() {
                        return Err(invalid(format!("{id}.{k}: {} has no anchor `{}`", a.entity, a.anchor)));
                    }
                }
                _ => {}
            }
        }
        if !is_builtin(&e.type_id)
            && let Ok(def) = plugin_for(registry, e)
        {
            for (k, pd) in &def.def.props {
                if let PropDef::Ref { required: true, .. } = pd
                    && !e.props.contains_key(k)
                {
                    return Err(invalid(format!("{id}: required reference `{k}` is missing")));
                }
            }
        }
        e.style.validate().map_err(|x| invalid(format!("{id}: {x}")))?;
    }
    // Constraints changed by the transaction.
    for (cid, c) in &ov.constraints {
        let Some(c) = c else { continue };
        for e in c.rule.entities() {
            if ov.entity(e).is_none() {
                return Err(invalid(format!("{cid} references missing {e}")));
            }
        }
        for a in c.rule.anchors() {
            if evaluate(ctx, a.entity).anchor(&a.anchor).is_none() {
                return Err(invalid(format!("{cid}: {} has no anchor `{}`", a.entity, a.anchor)));
            }
        }
    }
    // Nothing may still reference deleted entities.
    if !deleted.is_empty() {
        for cid in ov.constraint_ids() {
            if let Some(c) = ov.constraint(cid)
                && (c.rule.entities().iter().any(|e| deleted.contains(e))
                    || c.owner.is_some_and(|o| deleted.contains(&o)))
            {
                return Err(invalid(format!("{cid} references a deleted entity")));
            }
        }
        for id in ov.entity_ids() {
            if let Some(e) = ov.entity(id)
                && e.referenced_entities().any(|r| deleted.contains(&r))
            {
                return Err(invalid(format!("{id} references a deleted entity")));
            }
        }
    }
    // Groups: members exist, one parent, no cycles.
    if !ov.groups.is_empty() || !deleted.is_empty() {
        let mut parent_of_entity: BTreeMap<EntityId, GroupId> = BTreeMap::new();
        let mut parent_of_group: BTreeMap<GroupId, GroupId> = BTreeMap::new();
        for gid in ov.group_ids() {
            let Some(g) = ov.group(gid) else { continue };
            for m in &g.members {
                if ov.entity(*m).is_none() {
                    return Err(invalid(format!("{gid} contains missing {m}")));
                }
                if parent_of_entity.insert(*m, gid).is_some() {
                    return Err(invalid(format!("{m} is in more than one group")));
                }
            }
            for c in &g.children {
                if parent_of_group.insert(*c, gid).is_some() {
                    return Err(invalid(format!("{c} has more than one parent group")));
                }
            }
        }
        for gid in ov.group_ids() {
            let mut cur = gid;
            for _ in 0..=parent_of_group.len() {
                match parent_of_group.get(&cur) {
                    Some(p) if *p == gid => return Err(invalid(format!("group cycle through {gid}"))),
                    Some(p) => cur = *p,
                    None => break,
                }
            }
        }
    }
    let _ = notes;
    Ok(())
}

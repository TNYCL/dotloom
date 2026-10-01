//! Turning a transaction overlay into a history [`Change`].

use std::collections::BTreeSet;

use dotloom_document::EntityId;

use crate::{DocView, Overlay, history::Change, view::OrderOp};

/// Final draw order of the overlay.
fn final_order(ov: &Overlay<'_>) -> Vec<EntityId> {
    let mut order: Vec<EntityId> =
        ov.base.order().iter().copied().filter(|id| !matches!(ov.entities.get(id), Some(None))).collect();
    let present: BTreeSet<EntityId> = order.iter().copied().collect();
    for (id, e) in &ov.entities {
        if e.is_some() && !present.contains(id) {
            order.push(*id);
        }
    }
    for op in &ov.order_ops {
        match *op {
            OrderOp::Move(id, index) => {
                if let Some(pos) = order.iter().position(|x| *x == id) {
                    order.remove(pos);
                    let i = index.min(order.len());
                    order.insert(i, id);
                }
            }
        }
    }
    order
}

/// Before/after states of everything the overlay changed.
#[must_use]
pub fn change_of(ov: &Overlay<'_>) -> Change {
    let mut change = Change::default();
    let order = final_order(ov);
    let mut touched: BTreeSet<EntityId> = ov.entities.keys().copied().collect();
    for op in &ov.order_ops {
        let OrderOp::Move(id, _) = op;
        touched.insert(*id);
    }
    for id in touched {
        let before = ov.base.entity(id).cloned().zip(ov.base.order_index(id));
        let after = ov.entity(id).cloned().zip(order.iter().position(|x| *x == id));
        if before != after {
            change.entities.push((id, before, after));
        }
    }
    for (id, after) in &ov.constraints {
        let before = ov.base.constraint(*id).cloned();
        if before != *after {
            change.constraints.push((*id, before, after.clone()));
        }
    }
    for (id, after) in &ov.groups {
        let before = ov.base.group(*id).cloned();
        if before != *after {
            change.groups.push((*id, before, after.clone()));
        }
    }
    if let Some(l) = &ov.layers
        && l.as_slice() != ov.base.layers()
    {
        change.layers = Some((ov.base.layers().to_vec(), l.clone()));
    }
    if let Some(s) = &ov.settings
        && *s != ov.base.settings
    {
        change.settings = Some((ov.base.settings.clone(), s.clone()));
    }
    change
}

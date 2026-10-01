//! Plugin entity property migrations (type version upgrades).

use dotloom_document::{Document, EntityId, PropValue};

use crate::registry::Registry;

/// Upgrade plugin entities whose `type_version` is older than the registered
/// definition, applying the definition's migration steps in order. Entities written
/// by a newer version are left untouched (they evaluate as read-only).
pub fn migrate_entities(doc: &mut Document, registry: &Registry) -> Vec<(EntityId, u32, u32)> {
    let mut done = Vec::new();
    let ids: Vec<EntityId> = doc.order().to_vec();
    for id in ids {
        let Some(e) = doc.entity(id) else { continue };
        let Some(entry) = registry.get(&e.type_id) else { continue };
        let def = &entry.compiled.def;
        if e.type_version >= def.version {
            continue;
        }
        let from = e.type_version;
        let mut props = e.props.clone();
        let mut v = from;
        while v < def.version {
            if let Some(step) = def.migrations.iter().find(|m| m.from == v) {
                for (old, new) in &step.rename {
                    if let Some(val) = props.remove(old) {
                        props.insert(new.clone(), val);
                    }
                }
                for (k, val) in &step.set {
                    if !props.contains_key(k)
                        && let Ok(pv) = serde_json::from_value::<PropValue>(val.clone())
                    {
                        props.insert(k.clone(), pv);
                    }
                }
                for k in &step.remove {
                    props.remove(k);
                }
            }
            v += 1;
        }
        if let Some(e) = doc.entity_mut(id) {
            e.props = props;
            e.type_version = def.version;
            done.push((id, from, def.version));
        }
    }
    done
}

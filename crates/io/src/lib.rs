//! # dotloom-io
//!
//! File formats for Dotloom:
//!
//! * `.dotl` project files (ZIP container with `manifest.json`, `document.json`,
//!   optional `view.json` and content-addressed `assets/`), read with a bounded
//!   parser and written atomically on native platforms;
//! * SVG and ASCII DXF import/export with explicit [`ConversionReport`]s listing
//!   what each format cannot carry.
//!
//! PNG export lives in `dotloom-render` (it needs a GPU adapter).

pub mod dotl;
pub mod dxf;
mod report;
pub mod svg;
pub mod zip;

use dotloom_engine::{
    ApplyOptions, Command, CommitReport, Engine, EngineError, NewEntity, Transaction,
    document::{Color, LayerId},
};

pub use dotl::{DotlError, DotlFile, DotlLimits, LoadReport, Manifest, read_dotl, write_dotl};
pub use report::{ConversionReport, Loss, LossKind};

/// Add imported layers and entities to an engine in one transaction (one undo step).
pub fn import_into(
    engine: &mut Engine,
    label: &str,
    layers: &[(String, Option<Color>)],
    entities: Vec<(usize, NewEntity)>,
) -> Result<CommitReport, EngineError> {
    let ids = engine.reserve_ids(layers.len());
    let mut commands = Vec::with_capacity(layers.len() + entities.len());
    let mut layer_ids = Vec::with_capacity(layers.len());
    for ((name, color), id) in layers.iter().zip(ids) {
        let lid = LayerId(id);
        layer_ids.push(lid);
        commands.push(Command::AddLayer { id: Some(lid), name: name.clone() });
        if let Some(c) = color {
            commands.push(Command::UpdateLayer {
                id: lid,
                patch: dotloom_engine::LayerPatch { color: Some(*c), ..Default::default() },
            });
        }
    }
    for (li, mut e) in entities {
        e.layer = layer_ids.get(li).copied();
        commands.push(Command::CreateEntity { id: None, entity: e });
    }
    engine.apply(Transaction::new(label, commands), ApplyOptions::default())
}

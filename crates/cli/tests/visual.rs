//! Visual regression (DL-TEST-9): documents rendered by the wgpu renderer compared
//! with reviewed baselines in `tests/fixtures/visual/`.
//!
//! The baselines belong to one fixed environment — Mesa lavapipe (Vulkan, CPU) on
//! the CI Linux runner, the embedded Inter subset, 4× MSAA — because GPUs differ in
//! anti-aliasing and rasterization rules. The test therefore refuses other adapters
//! instead of passing on them, and is `#[ignore]`d so a plain `cargo test` never
//! reports it as passed:
//!
//! ```text
//! WGPU_BACKEND=vulkan cargo test -p dotloom-cli --features png --test visual -- --ignored
//! ```
//!
//! Tolerance: a pixel differs when a channel differs by more than 24/255; a case
//! fails when more than 0.2 % of its pixels differ. On failure the actual image and
//! a difference image are written to `target/visual/`. `DOTLOOM_VISUAL_UPDATE=1`
//! writes new baselines (review them before committing); `DOTLOOM_VISUAL_PREVIEW=1`
//! renders on any adapter into `target/visual/preview/` without comparing.
#![cfg(feature = "png")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use dotloom_engine::{ApplyOptions, Command, Engine, EntityTypeDef, Transaction, document::EntityId};
use dotloom_render::{Grid, Headless, Image, RendererOptions, Theme, View};
use serde_json::{Value, json};

const CHANNEL_TOLERANCE: u8 = 24;
const MAX_DIFFERENT: f64 = 0.002;

struct Case {
    name: &'static str,
    width: f64,
    height: f64,
    dpr: f64,
    dark: bool,
    grid: bool,
    /// Camera scale relative to the fitted view.
    zoom: f64,
    hover: Option<u64>,
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn apply(e: &mut Engine, commands: Value) -> Vec<EntityId> {
    let commands: Vec<Command> = serde_json::from_value(commands).unwrap();
    e.apply(Transaction::new("fixture", commands), ApplyOptions::default()).unwrap().created
}

fn plugins(e: &mut Engine, name: &str) {
    let text = std::fs::read_to_string(root().join(format!("tests/fixtures/plugins/{name}.json"))).unwrap();
    let v: Value = serde_json::from_str(&text).unwrap();
    let defs: Vec<EntityTypeDef> = match v {
        Value::Array(_) => serde_json::from_value(v).unwrap(),
        other => vec![serde_json::from_value(other).unwrap()],
    };
    for d in defs {
        e.register_type(d, name).unwrap();
    }
}

/// Lines (widths, dashes), bulged and filled polylines, rectangle, circle, arc,
/// polygon with a hole, Bézier path, Latin and Turkish text (straight and rotated).
fn primitives() -> Engine {
    let mut e = Engine::default();
    let stroke = |w: f64| json!({ "stroke": "#1f2937ff", "strokeWidth": w });
    apply(
        &mut e,
        json!([
            { "op": "createEntity", "entity": { "geometry": { "type": "line", "a": [0, 0], "b": [200, 0] }, "style": stroke(1.0) } },
            { "op": "createEntity", "entity": { "geometry": { "type": "line", "a": [0, 20], "b": [200, 20] }, "style": stroke(3.0) } },
            { "op": "createEntity", "entity": { "geometry": { "type": "line", "a": [0, 40], "b": [200, 40] }, "style": { "stroke": "#b91c1cff", "strokeWidth": 2, "dash": [8, 4] } } },
            { "op": "createEntity", "entity": { "geometry": { "type": "polyline", "points": [[0, 70], [60, 70], [120, 70], [180, 100]], "bulges": [0, 1, 0] }, "style": stroke(2.0) } },
            { "op": "createEntity", "entity": { "geometry": { "type": "polyline", "points": [[220, 0], [300, 0], [300, 60], [260, 90], [220, 60]], "closed": true }, "style": { "stroke": "#065f46ff", "fill": "#10b98166", "strokeWidth": 2 } } },
            { "op": "createEntity", "entity": { "geometry": { "type": "rect", "origin": [320, 0], "width": 90, "height": 60 }, "style": { "stroke": "#1e3a8aff", "fill": "#3b82f640", "strokeWidth": 1.5 } } },
            { "op": "createEntity", "entity": { "geometry": { "type": "circle", "center": [470, 30], "radius": 30 }, "style": stroke(2.0) } },
            { "op": "createEntity", "entity": { "geometry": { "type": "arc", "center": [560, 30], "radius": 30, "start": 0.3, "sweep": 4.5 }, "style": stroke(2.0) } },
            { "op": "createEntity", "entity": { "geometry": { "type": "polygon", "outer": [[220, 120], [340, 120], [340, 200], [220, 200]], "holes": [[[250, 140], [310, 140], [310, 180], [250, 180]]] }, "style": { "stroke": "#7c2d12ff", "fill": "#f9731680", "strokeWidth": 1 } } },
            { "op": "createEntity", "entity": { "geometry": { "type": "path", "elements": [{ "M": [370, 200] }, { "C": [[400, 120], [460, 260], [500, 140]] }, { "Q": [[540, 100], [590, 180]] }] }, "style": stroke(2.0) } },
            { "op": "createEntity", "entity": { "geometry": { "type": "text", "position": [0, 130], "content": "Dotloom 0123 ABC xyz", "height": 14 } } },
            { "op": "createEntity", "entity": { "geometry": { "type": "text", "position": [0, 160], "content": "Ölçü ğüşıİç ÇĞÖŞÜ", "height": 14 } } },
            { "op": "createEntity", "entity": { "geometry": { "type": "text", "position": [40, 230], "content": "rotated 15°", "height": 10, "rotation": 0.2618 } } }
        ]),
    );
    e
}

/// Large Turkish text: dotted/dotless I in both cases and every accented letter.
fn turkish() -> Engine {
    let mut e = Engine::default();
    apply(
        &mut e,
        json!([
            { "op": "createEntity", "entity": { "geometry": { "type": "text", "position": [0, 60], "content": "İI ıi Iİ", "height": 40 } } },
            { "op": "createEntity", "entity": { "geometry": { "type": "text", "position": [0, 0], "content": "ÇĞÖŞÜ çğöşü", "height": 40 } } }
        ]),
    );
    e
}

/// The floor plan plugin: four walls, a door hosted by a wall.
fn floorplan() -> Engine {
    let mut e = Engine::default();
    plugins(&mut e, "floorplan");
    let corners = [[0, 0], [5000, 0], [5000, 3500], [0, 3500]];
    let walls: Vec<Value> = (0..4)
        .map(|i| {
            json!({ "op": "createEntity", "entity": { "type": "floorplan.wall",
                "props": { "start": corners[i], "end": corners[(i + 1) % 4] } } })
        })
        .collect();
    let ids = apply(&mut e, Value::Array(walls));
    apply(
        &mut e,
        json!([{ "op": "createEntity", "entity": { "type": "floorplan.door",
            "props": { "host": { "ref": ids[0].0 }, "offset": 1200, "width": 900 } } }]),
    );
    e
}

/// 2500 small shapes, zoomed out: level-of-detail tessellation.
fn dense() -> Engine {
    let mut e = Engine::default();
    let mut cmds = Vec::new();
    for i in 0..2500 {
        let (x, y) = (f64::from(i % 50) * 40.0, f64::from(i / 50) * 40.0);
        let geometry = if i % 2 == 0 {
            json!({ "type": "circle", "center": [x + 20.0, y + 20.0], "radius": 15 })
        } else {
            json!({ "type": "line", "a": [x + 5.0, y + 5.0], "b": [x + 35.0, y + 35.0] })
        };
        cmds.push(json!({ "op": "createEntity", "entity": { "geometry": geometry } }));
    }
    apply(&mut e, Value::Array(cmds));
    e
}

fn render(e: &mut Engine, c: &Case) -> Image {
    let delta = e.full_scene();
    let bounds = delta
        .upserts
        .iter()
        .map(|i| i.bbox)
        .fold(dotloom_engine::geometry::Aabb::EMPTY, dotloom_engine::geometry::Aabb::union);
    let mut view =
        View { center: [0.0, 0.0], scale: 1.0, width: c.width, height: c.height, dpr: c.dpr }.fit(bounds, 24.0);
    view.scale *= c.zoom;
    let mut h = Headless::new(RendererOptions::default()).expect("GPU adapter");
    let adapter = h.adapter().clone();
    assert!(
        adapter.name.contains("llvmpipe") || preview(),
        "visual baselines belong to Mesa lavapipe (CI Linux, WGPU_BACKEND=vulkan); this adapter is {adapter:?}"
    );
    let r = h.renderer();
    r.set_theme(if c.dark { Theme::dark() } else { Theme::light() });
    r.set_grid(Grid { visible: c.grid, ..Grid::default() });
    r.set_view(view).unwrap();
    r.apply_delta(delta);
    r.set_hover(c.hover);
    h.render().unwrap().0
}

fn decode(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::EXPAND);
    let mut reader = dec.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    buf.truncate(info.buffer_size());
    (info.width, info.height, buf)
}

/// `DOTLOOM_VISUAL_PREVIEW=1`: render on any adapter into `target/visual/preview/`
/// to look at the fixtures; nothing is compared and the test fails on purpose.
fn preview() -> bool {
    std::env::var("DOTLOOM_VISUAL_PREVIEW").as_deref() == Ok("1")
}

fn compare(name: &str, img: &Image) -> Option<String> {
    let base = root().join(format!("tests/fixtures/visual/{name}.png"));
    let out = root().join("target/visual");
    if preview() {
        std::fs::create_dir_all(out.join("preview")).unwrap();
        std::fs::write(out.join(format!("preview/{name}.png")), img.to_png().unwrap()).unwrap();
        return Some(format!("{name}: preview only, not compared"));
    }
    if std::env::var("DOTLOOM_VISUAL_UPDATE").as_deref() == Ok("1") {
        std::fs::create_dir_all(base.parent().unwrap()).unwrap();
        std::fs::write(&base, img.to_png().unwrap()).unwrap();
        return None;
    }
    let Ok(bytes) = std::fs::read(&base) else {
        return Some(format!("{name}: no baseline at {}", base.display()));
    };
    let (w, h, want) = decode(&bytes);
    if (w, h) != (img.width, img.height) {
        return Some(format!("{name}: size {}x{} vs baseline {w}x{h}", img.width, img.height));
    }
    let mut diff = vec![0u8; want.len()];
    let mut different = 0usize;
    for (i, (a, b)) in img.pixels.as_chunks::<4>().0.iter().zip(want.as_chunks::<4>().0).enumerate() {
        let d = a.iter().zip(b).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0);
        if d > CHANNEL_TOLERANCE {
            different += 1;
            diff[i * 4..i * 4 + 4].copy_from_slice(&[255, 0, 0, 255]);
        } else {
            diff[i * 4..i * 4 + 4].copy_from_slice(&[a[0] / 3 + 170, a[1] / 3 + 170, a[2] / 3 + 170, 255]);
        }
    }
    let fraction = different as f64 / f64::from(w * h);
    if fraction <= MAX_DIFFERENT {
        return None;
    }
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join(format!("{name}.actual.png")), img.to_png().unwrap()).unwrap();
    let diff_img = Image { width: w, height: h, pixels: diff };
    std::fs::write(out.join(format!("{name}.diff.png")), diff_img.to_png().unwrap()).unwrap();
    Some(format!("{name}: {:.3} % of pixels differ (limit {:.1} %)", fraction * 100.0, MAX_DIFFERENT * 100.0))
}

#[test]
#[ignore = "baselines belong to Mesa lavapipe on CI Linux; run with --ignored (see module docs)"]
fn renders_match_reviewed_baselines() {
    let base =
        |name, dpr, dark, grid, zoom, hover| Case { name, width: 640.0, height: 400.0, dpr, dark, grid, zoom, hover };
    let mut failures = Vec::new();
    let mut prim = primitives();
    for c in [
        base("primitives", 1.0, false, false, 1.0, None),
        base("primitives-dpr2", 2.0, false, false, 1.0, None),
        base("primitives-zoom", 1.0, false, true, 4.0, None),
    ] {
        failures.extend(compare(c.name, &render(&mut prim, &c)));
    }
    // Dark theme with a selection and a hovered entity.
    let ids: Vec<EntityId> = prim.document().order().to_vec();
    prim.set_selection(&ids[..3]);
    let c = base("primitives-dark-selected", 1.0, true, true, 1.0, ids.get(6).map(|i| i.0));
    failures.extend(compare(c.name, &render(&mut prim, &c)));
    let c = base("text-turkish", 1.0, false, false, 1.0, None);
    failures.extend(compare(c.name, &render(&mut turkish(), &c)));
    let c = base("floorplan", 1.0, false, true, 1.0, None);
    failures.extend(compare(c.name, &render(&mut floorplan(), &c)));
    let c = base("dense-lod", 1.0, false, false, 0.5, None);
    failures.extend(compare(c.name, &render(&mut dense(), &c)));
    assert!(failures.is_empty(), "visual differences (see target/visual/):\n{}", failures.join("\n"));
}

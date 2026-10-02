//! Offscreen rendering on a real wgpu adapter (DL-RENDER-1/4/6/7).
//!
//! These tests need a GPU or software adapter (CI installs Mesa lavapipe). If none
//! is available they fail, unless `DOTLOOM_ALLOW_NO_GPU=1` is set, in which case
//! they print `SKIPPED (no GPU adapter)` — a skip is never reported as a pass.
#![cfg(feature = "png")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use dotloom_geometry::{Aabb, HAlign, Point, Rect, Segment, Shape, Text, VAlign};
use dotloom_render::scene::{Marker, MarkerKind, Overlay, Primitive, SceneDelta, SceneItem, Stroke};
use dotloom_render::{Grid, Headless, Image, RendererOptions, Theme, View};

fn headless(sample_count: u32) -> Option<Headless> {
    match Headless::new(RendererOptions { sample_count }) {
        Ok(h) => {
            eprintln!("adapter: {:?}", h.adapter());
            Some(h)
        }
        Err(e) if std::env::var("DOTLOOM_ALLOW_NO_GPU").as_deref() == Ok("1") => {
            eprintln!("SKIPPED (no GPU adapter): {e}");
            None
        }
        Err(e) => panic!("no GPU adapter ({e}); set DOTLOOM_ALLOW_NO_GPU=1 to skip explicitly"),
    }
}

fn px(img: &Image, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * img.width + x) * 4) as usize;
    [img.pixels[i], img.pixels[i + 1], img.pixels[i + 2], img.pixels[i + 3]]
}

fn is_bg(p: [u8; 4]) -> bool {
    p[0] > 245 && p[1] > 245 && p[2] > 245
}

fn item(id: u64, shape: Shape, stroke: Option<Stroke>, fill: Option<u32>) -> SceneItem {
    SceneItem { id, layer: 0, bbox: shape.bbox(), flags: 0, prims: vec![Primitive::Shape { shape, stroke, fill }] }
}

fn black(width: f32) -> Option<Stroke> {
    Some(Stroke { color: 0x0000_00ff, width, dash: vec![] })
}

fn scene() -> SceneDelta {
    let line = item(1, Shape::Line(Segment::new(Point::new(-40.0, 0.0), Point::new(40.0, 0.0))), black(2.0), None);
    let rect = item(
        2,
        Shape::Rect(Rect { origin: Point::new(10.0, 10.0), width: 20.0, height: 10.0 }),
        None,
        Some(0xff00_00ff),
    );
    let text = SceneItem {
        id: 3,
        layer: 0,
        bbox: Aabb::from_corners(Point::new(-40.0, -30.0), Point::new(0.0, -20.0)),
        flags: 0,
        prims: vec![Primitive::Text {
            text: Text {
                position: Point::new(-40.0, -30.0),
                content: "Ölçü ığş İ".into(),
                height: 8.0,
                rotation: 0.0,
                halign: HAlign::Left,
                valign: VAlign::Baseline,
            },
            color: 0x0000_00ff,
        }],
    };
    SceneDelta { revision: 1, upserts: vec![line, rect, text], order: Some(vec![1, 2, 3]), ..SceneDelta::default() }
}

fn setup(h: &mut Headless, dpr: f64) {
    let r = h.renderer();
    r.set_theme(Theme::light());
    r.set_grid(Grid { visible: false, ..Grid::default() });
    r.set_view(View { center: [0.0, 0.0], scale: 2.0, width: 200.0, height: 150.0, dpr }).unwrap();
    r.apply_delta(scene());
}

#[test]
fn draws_lines_fills_and_text() {
    let Some(mut h) = headless(4) else { return };
    setup(&mut h, 1.0);
    let (img, stats) = h.render().unwrap();
    assert_eq!((img.width, img.height), (200, 150));
    // Background corner.
    assert!(is_bg(px(&img, 2, 2)));
    // Horizontal line through the center (y = 75), 2 px wide.
    let on = px(&img, 100, 75);
    assert!(on[0] < 80 && on[1] < 80 && on[2] < 80, "line pixel {on:?}");
    assert!(is_bg(px(&img, 100, 70)), "line is thin");
    // Red rectangle: world (10..30, 10..20) → css x 120..160, y 35..55.
    let r = px(&img, 140, 45);
    assert!(r[0] > 230 && r[1] < 30 && r[2] < 30, "fill pixel {r:?}");
    // Text region (world x −40..0, baseline y −30 → css y 135) contains ink.
    let ink = (20..100).flat_map(|x| (118..136).map(move |y| (x, y))).filter(|&(x, y)| !is_bg(px(&img, x, y))).count();
    assert!(ink > 60, "text ink pixels: {ink}");
    assert!(stats.glyphs >= 8, "{stats:?}");
    assert_eq!(stats.chunks_drawn, 1);
}

#[test]
fn device_pixel_ratio_has_no_double_scaling() {
    let Some(mut h) = headless(1) else { return };
    setup(&mut h, 2.0);
    let (img, _) = h.render().unwrap();
    assert_eq!((img.width, img.height), (400, 300));
    // The same CSS position maps to 2× device pixels.
    let r = px(&img, 280, 90);
    assert!(r[0] > 230 && r[1] < 30, "fill at 2× {r:?}");
    let line = px(&img, 200, 150);
    assert!(line[0] < 80, "line at 2× {line:?}");
    // Line width is 2 CSS px = 4 device px.
    let dark = (140..160).filter(|&y| px(&img, 200, y)[0] < 128).count();
    assert!((3..=5).contains(&dark), "device line width {dark}");
}

#[test]
fn culling_hides_offscreen_items_and_overlays_draw_on_top() {
    let Some(mut h) = headless(1) else { return };
    setup(&mut h, 1.0);
    let r = h.renderer();
    r.set_overlay(Overlay {
        markers: vec![Marker { at: Point::new(-30.0, 20.0), kind: MarkerKind::Endpoint }],
        ..Overlay::default()
    });
    let (img, _) = h.render().unwrap();
    // Endpoint marker: 10 px square outline around css (40, 35).
    let edge = px(&img, 35, 35);
    assert!(!is_bg(edge), "marker outline {edge:?}");
    // Move the camera far away: nothing is drawn, chunk culled.
    h.renderer()
        .set_view(View { center: [5000.0, 5000.0], scale: 2.0, width: 200.0, height: 150.0, dpr: 1.0 })
        .unwrap();
    h.renderer().set_overlay(Overlay::default());
    let (img, stats) = h.render().unwrap();
    assert_eq!(stats.chunks_drawn, 0);
    assert!(img.pixels.as_chunks::<4>().0.iter().all(|p| is_bg(*p)));
}

#[test]
fn selection_and_dark_theme() {
    let Some(mut h) = headless(1) else { return };
    setup(&mut h, 1.0);
    let mut sel = scene();
    sel.upserts.retain(|i| i.id == 1);
    sel.upserts[0].flags = dotloom_render::scene::flags::SELECTED;
    sel.order = None;
    h.renderer().apply_delta(sel);
    h.renderer().set_theme(Theme::dark());
    let (img, _) = h.render().unwrap();
    let bg = px(&img, 2, 2);
    assert!(bg[0] < 40 && bg[1] < 40, "dark background {bg:?}");
    // Selected line uses the selection color (blue-ish).
    let l = px(&img, 100, 75);
    assert!(l[2] > l[0] + 60, "selection color {l:?}");
}

#[test]
fn png_encoding() {
    let Some(mut h) = headless(4) else { return };
    setup(&mut h, 1.0);
    let (img, _) = h.render().unwrap();
    let png = img.to_png().unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
}

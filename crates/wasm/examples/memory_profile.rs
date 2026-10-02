//! Heap profile of opening a large `.dotl` (DL-PERF-8), natively, with the same steps
//! as the WebAssembly binding: parse the container, load the engine, encode the full
//! scene for the renderer.
//!
//! ```text
//! cargo run --release -p dotloom-wasm --example memory_profile -- 100000
//! ```
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use dotloom_engine::{ApplyOptions, Command, Engine, Transaction};
use dotloom_io::dotl::{DotlFile, DotlLimits, read_dotl, write_dotl};

struct Counting;

static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every call is forwarded unchanged to the system allocator; only the byte
// counters are updated around it.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: same contract as `GlobalAlloc::alloc`, forwarded.
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            let now = CURRENT.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(now, Ordering::Relaxed);
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: same contract as `GlobalAlloc::dealloc`, forwarded.
        unsafe { System.dealloc(ptr, layout) };
        CURRENT.fetch_sub(layout.size(), Ordering::Relaxed);
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

fn mib(b: usize) -> f64 {
    b as f64 / 1_048_576.0
}

fn reset_peak() {
    PEAK.store(CURRENT.load(Ordering::Relaxed), Ordering::Relaxed);
}

fn report(stage: &str, before: usize, n: usize) {
    let cur = CURRENT.load(Ordering::Relaxed);
    let peak = PEAK.load(Ordering::Relaxed);
    println!(
        "{stage:<28} live {:>8.1} MiB ({:>6.0} B/entity)  peak during stage {:>8.1} MiB",
        mib(cur),
        (cur.saturating_sub(before)) as f64 / n as f64,
        mib(peak),
    );
}

fn shapes(count: usize, first: usize, batch: usize) -> Vec<Command> {
    let cols = (count as f64).sqrt().ceil() as usize;
    let mut out = Vec::new();
    for i in first..count.min(first + batch) {
        let x = (i % cols) as f64 * 100.0;
        let y = (i / cols) as f64 * 100.0;
        let geometry = match i % 10 {
            0..=3 => serde_json::json!({"type": "line", "a": [x + 10.0, y + 10.0], "b": [x + 90.0, y + 70.0]}),
            4 | 5 => serde_json::json!({"type": "rect", "origin": [x + 15.0, y + 15.0], "width": 60, "height": 45}),
            6 | 7 => serde_json::json!({"type": "circle", "center": [x + 50.0, y + 50.0], "radius": 30}),
            8 => {
                serde_json::json!({"type": "arc", "center": [x + 50.0, y + 50.0], "radius": 35, "start": 0.3, "sweep": 4.2})
            }
            _ => {
                serde_json::json!({"type": "polyline", "points": [[x + 10.0, y + 20.0], [x + 30.0, y + 80.0], [x + 50.0, y + 20.0]]})
            }
        };
        let mut entity = serde_json::json!({ "geometry": geometry });
        if i % 5 == 4 {
            entity["style"] = serde_json::json!({ "fill": "#4f8ad433" });
        }
        if let Ok(c) = serde_json::from_value(serde_json::json!({ "op": "createEntity", "entity": entity })) {
            out.push(c);
        }
    }
    out
}

fn main() {
    let n: usize = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(100_000);
    let bytes = {
        let mut e = Engine::default();
        for first in (0..n).step_by(10_000) {
            if let Err(err) = e.apply(Transaction::new("build", shapes(n, first, 10_000)), ApplyOptions::default()) {
                eprintln!("build: {err}");
                return;
            }
        }
        match write_dotl(&DotlFile::new(e.document().clone())) {
            Ok(b) => b,
            Err(err) => {
                eprintln!("save: {err}");
                return;
            }
        }
    };
    println!("{n} entities, .dotl {:.1} MiB", mib(bytes.len()));
    if std::env::var("SIZES").is_ok() {
        use core::mem::size_of;
        println!(
            "size_of Evaluated {} Drawable {} Shape {} Anchor {} PrimStyle {} Entity {}",
            size_of::<dotloom_engine::eval::Evaluated>(),
            size_of::<dotloom_engine::eval::Drawable>(),
            size_of::<dotloom_engine::geometry::Shape>(),
            size_of::<dotloom_engine::geometry::Anchor>(),
            size_of::<dotloom_engine::registry::PrimStyle>(),
            size_of::<dotloom_engine::document::Entity>(),
        );
    }
    let base = CURRENT.load(Ordering::Relaxed);
    reset_peak();

    let (mut file, _) = match read_dotl(&bytes, &DotlLimits::default()) {
        Ok(f) => f,
        Err(err) => {
            eprintln!("read: {err}");
            return;
        }
    };
    report("read_dotl (document)", base, n);

    reset_peak();
    let mut engine = Engine::default();
    let doc = std::mem::replace(&mut file.document, dotloom_engine::document::Document::new());
    if let Err(err) = engine.load(doc) {
        eprintln!("load: {err}");
        return;
    }
    report("engine.load", base, n);

    reset_peak();
    let scene = engine.full_scene().encode();
    report("full scene encoded", base, n);
    println!("full scene buffer {:.1} MiB", mib(scene.len()));
    drop(scene);
    report("after scene buffer freed", base, n);
}

//! Fuzz corpus replay on stable (DL-TEST-10).
//!
//! The committed seeds (`fuzz/seeds/<target>/`) and every input that once crashed
//! (`fuzz/regressions/<target>/`) go through the same harness as the cargo-fuzz
//! targets (`fuzz/harness.rs`), plus a bounded number of deterministic mutations of
//! each seed (bit flips, truncations, insertions, splices; fixed xorshift seed) — a
//! small fuzz smoke in every CI run. A failing mutation is written to
//! `target/fuzz-failures/` for reproduction; add it to `fuzz/regressions/` once the
//! bug is fixed.
//!
//! The generated seeds (containers, document JSON, scene buffers, transactions,
//! plugin definitions, SVG) are rewritten by
//! `cargo test -p dotloom-cli --test fuzz_replay write_generated_seeds -- --ignored`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

include!("../../../fuzz/harness.rs");

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

type Target = (&'static str, fn(&[u8]));

const TARGETS: [Target; 7] = [
    ("dotl", dotl),
    ("document_json", document_json),
    ("svg", svg),
    ("dxf", dxf),
    ("transaction", transaction),
    ("plugin", plugin),
    ("scene", scene),
];

/// Mutations per seed and target in every run.
const MUTATIONS: usize = 200;

fn fuzz_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fuzz")
}

fn files(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<_> = rd
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .map(|p| {
            let b = std::fs::read(&p).unwrap();
            (p, b)
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn survives(f: fn(&[u8]), data: &[u8]) -> bool {
    catch_unwind(AssertUnwindSafe(|| f(data))).is_ok()
}

#[test]
fn seeds_and_regressions_are_handled() {
    let mut failures = Vec::new();
    let mut count = 0;
    for (name, f) in TARGETS {
        let seeds = files(&fuzz_dir().join("seeds").join(name));
        assert!(!seeds.is_empty(), "no seeds for {name}");
        for (path, data) in seeds.iter().chain(files(&fuzz_dir().join("regressions").join(name)).iter()) {
            count += 1;
            if !survives(f, data) {
                failures.push(path.display().to_string());
            }
        }
    }
    assert!(failures.is_empty(), "inputs that panic:\n{}", failures.join("\n"));
    assert!(count >= TARGETS.len());
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
}

fn mutate(rng: &mut Rng, seed: &[u8], other: &[u8]) -> Vec<u8> {
    let mut d = seed.to_vec();
    for _ in 0..=rng.below(3) {
        match rng.below(5) {
            0 if !d.is_empty() => {
                let i = rng.below(d.len());
                d[i] ^= 1 << rng.below(8);
            }
            1 => d.truncate(rng.below(d.len() + 1)),
            2 => {
                let i = rng.below(d.len() + 1);
                let bytes: Vec<u8> = (0..1 + rng.below(8)).map(|_| rng.next() as u8).collect();
                d.splice(i..i, bytes);
            }
            3 if !other.is_empty() => {
                // Splice a slice of another seed of the same target.
                let a = rng.below(other.len());
                let b = (a + 1 + rng.below(64)).min(other.len());
                let i = rng.below(d.len() + 1);
                d.splice(i..i, other[a..b].iter().copied());
            }
            _ if !d.is_empty() => {
                // Interesting values: 0, 0xff, digits and JSON/XML punctuation.
                let i = rng.below(d.len());
                d[i] = *[0u8, 0xff, b'9', b'-', b'"', b'{', b'<', b'.'].get(rng.below(8)).unwrap_or(&0);
            }
            _ => {}
        }
    }
    d
}

#[test]
fn deterministic_mutations_are_handled() {
    let mut rng = Rng(0x05ee_dd07_1004);
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/fuzz-failures");
    let mut failures = Vec::new();
    for (name, f) in TARGETS {
        let seeds = files(&fuzz_dir().join("seeds").join(name));
        for (i, (path, seed)) in seeds.iter().enumerate() {
            let other = &seeds[(i + 1) % seeds.len()].1;
            for m in 0..MUTATIONS {
                let data = mutate(&mut rng, seed, other);
                if !survives(f, &data) {
                    std::fs::create_dir_all(&out).unwrap();
                    let file = out.join(format!("{name}-{i}-{m}.bin"));
                    std::fs::write(&file, &data).unwrap();
                    failures.push(format!("{} (mutation {m}) -> {}", path.display(), file.display()));
                }
            }
        }
    }
    assert!(failures.is_empty(), "mutated inputs that panic:\n{}", failures.join("\n"));
}

#[test]
#[ignore = "rewrites fuzz/seeds from the current code; run explicitly"]
fn write_generated_seeds() {
    use dotloom_io::dotl::{DotlFile, write_dotl};
    let seeds = fuzz_dir().join("seeds");
    let put = |dir: &str, name: &str, bytes: &[u8]| {
        let d = seeds.join(dir);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join(name), bytes).unwrap();
    };
    let mut e = engine_with_content();
    let doc = e.document().clone();
    put("dotl", "content.dotl", &write_dotl(&DotlFile::new(doc.clone())).unwrap());
    put("dotl", "empty.dotl", &write_dotl(&DotlFile::new(Document::new())).unwrap());
    put("document_json", "content.json", doc.to_json_string().unwrap().as_bytes());
    put("document_json", "empty.json", Document::new().to_json_string().unwrap().as_bytes());
    put("scene", "content.bin", &e.full_scene().encode());
    put("scene", "empty.bin", &SceneDelta::default().encode());
    put(
        "transaction",
        "draw.json",
        br#"[{"op":"createEntity","entity":{"geometry":{"type":"polyline","points":[[0,0],[50,0],[50,50]],"bulges":[0,1]}}},{"op":"addConstraint","constraint":{"rule":{"kind":"horizontal","a":{"entity":1000,"anchor":"start"},"b":{"entity":1000,"anchor":"end"}}}}]"#,
    );
    put(
        "transaction",
        "edit.json",
        br#"[{"op":"setParams","values":[{"entity":1000,"param":"b.x","value":140},{"entity":1005,"param":"width","value":1500}],"mode":"exact"},{"op":"deleteEntity","id":1002}]"#,
    );
    put(
        "transaction",
        "drag-like.json",
        br#"[{"op":"setParams","values":[{"entity":1001,"param":"b.y","value":-30}],"mode":"prefer"}]"#,
    );
    for text in [FLOORPLAN, SHELF] {
        let v: serde_json::Value = serde_json::from_str(text).unwrap();
        let defs = match v {
            serde_json::Value::Array(a) => a,
            other => vec![other],
        };
        for d in defs {
            let id = d["typeId"].as_str().unwrap().replace('.', "-");
            put("plugin", &format!("{id}.json"), serde_json::to_string_pretty(&d).unwrap().as_bytes());
        }
    }
    put(
        "svg",
        "shapes.svg",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 100" width="200mm" height="100mm"><g id="walls" transform="translate(10 10) rotate(5)"><rect x="0" y="0" width="80" height="40" fill="#ccc" stroke="#000"/><circle cx="120" cy="20" r="15"/><path d="M0 60 C 20 90 60 90 80 60 Q 100 40 120 60 A 20 20 0 0 1 160 60 Z"/></g><text x="10" y="95" font-size="8">Ölçü ğüş</text><polyline points="0,0 10,5 20,0"/><line x1="0" y1="0" x2="200" y2="100" stroke-dasharray="4 2"/></svg>"##.as_bytes(),
    );
    put(
        "svg",
        "hostile.svg",
        br##"<?xml version="1.0"?><!DOCTYPE svg [<!ENTITY x "y">]><svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script><use href="#a"/><image href="http://example.com/a.png"/><foreignObject><div/></foreignObject><rect id="a" width="10" height="10"/></svg>"##,
    );
    let dxf_fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/dxf");
    for (path, bytes) in files(&dxf_fixtures) {
        if path.extension().is_some_and(|e| e == "dxf") {
            put("dxf", &path.file_name().unwrap().to_string_lossy(), &bytes);
        }
    }
}

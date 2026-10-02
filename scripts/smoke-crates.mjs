#!/usr/bin/env node
// Crate consumer smoke test (DL-TEST-11, DL-OSS-5): package the publishable crates
// exactly as `cargo publish` would, then build and run a fresh project *outside the
// repository* that depends on them by version. `[patch.crates-io]` points those
// versions at the unpacked `.crate` contents (they are not on crates.io yet), so the
// consumer sees only what the packages contain — no workspace paths.
//
//   node scripts/smoke-crates.mjs [--work <dir outside the repo>] [--skip-package]

import { spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const arg = (name) => {
  const i = process.argv.indexOf(name)
  return i >= 0 ? process.argv[i + 1] : undefined
}
const work = arg('--work') ? resolve(arg('--work')) : mkdtempSync(join(tmpdir(), 'dotloom-crates-'))
if (!relative(root, work).startsWith('..')) {
  console.error(`✖ the work directory must be outside the repository: ${work}`)
  process.exit(1)
}
rmSync(work, { recursive: true, force: true })
mkdirSync(work, { recursive: true })

function run(cmd, args, cwd) {
  console.log(`\n▶ (${relative(root, cwd) || '.'}) ${cmd} ${args.join(' ')}`)
  const r = spawnSync(cmd, args, { cwd, stdio: 'inherit' })
  if (r.status !== 0) {
    console.error(`✖ ${cmd} failed`)
    process.exit(r.status ?? 1)
  }
}

// 1. Package (and verify) every publishable crate against each other.
const version = /^version\s*=\s*"([^"]+)"/m.exec(readFileSync(join(root, 'Cargo.toml'), 'utf8'))?.[1]
const PUBLISHED = [
  'dotloom-geometry',
  'dotloom-constraints',
  'dotloom-document',
  'dotloom-scene',
  'dotloom-engine',
  'dotloom-io',
  'dotloom-render',
  'dotloom-cli',
]
if (!process.argv.includes('--skip-package')) {
  run(
    'cargo',
    ['package', '--workspace', '--locked', '--exclude', 'dotloom-wasm', '--exclude', 'dotloom-render-web'],
    root,
  )
}
const pkgDir = join(root, 'target', 'package')
const unpacked = Object.fromEntries(PUBLISHED.map((c) => [c, join(pkgDir, `${c}-${version}`)]))
for (const [c, d] of Object.entries(unpacked)) {
  const files = readdirSync(d, { recursive: true }).map(String)
  if (!files.includes('Cargo.toml')) {
    console.error(`✖ ${c}: no unpacked package at ${d}`)
    process.exit(1)
  }
  for (const lic of ['LICENSE-MIT', 'LICENSE-APACHE']) {
    if (!files.includes(lic)) {
      console.error(`✖ ${c}: the package lacks ${lic}`)
      process.exit(1)
    }
  }
}

// 2. A consumer project outside the repository.
const app = join(work, 'consumer')
mkdirSync(join(app, 'src'), { recursive: true })
const toml = (p) => p.replaceAll('\\', '/')
writeFileSync(
  join(app, 'Cargo.toml'),
  `[package]
name = "dotloom-consumer"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
dotloom-engine = "=${version}"
dotloom-io = "=${version}"

[patch.crates-io]
${PUBLISHED.map((c) => `${c} = { path = "${toml(unpacked[c])}" }`).join('\n')}
`,
)
writeFileSync(
  join(app, 'src', 'main.rs'),
  `//! Uses the packaged crates the way a Rust application would.
use dotloom_engine::{
    ApplyOptions, Command, ConstraintSpec, Engine, NewEntity, Transaction,
    document::{AnchorRef, LineRef, RuleSpec},
    geometry::{Point, Segment, Shape},
};
use dotloom_io::{DotlFile, DotlLimits, read_dotl, svg::{SvgExportOptions, export_svg}, write_dotl};

fn main() {
    let mut e = Engine::default();
    let line = |a: (f64, f64), b: (f64, f64)| Command::CreateEntity {
        id: None,
        entity: NewEntity {
            geometry: Some(Shape::Line(Segment::new(Point::new(a.0, a.1), Point::new(b.0, b.1)))),
            ..NewEntity::default()
        },
    };
    let r = e
        .apply(Transaction::new("lines", vec![line((0.0, 0.0), (90.0, 5.0)), line((95.0, 0.0), (95.0, 40.0))]), ApplyOptions::default())
        .expect("create");
    let (a, b) = (r.created[0], r.created[1]);
    let rule = |rule| Command::AddConstraint {
        id: None,
        constraint: ConstraintSpec { rule, strength: Default::default(), enabled: true, label: None, source: None },
    };
    e.apply(
        Transaction::new(
            "rules",
            vec![
                rule(RuleSpec::FixPoint { a: AnchorRef::new(a, "start"), at: Point::ORIGIN }),
                rule(RuleSpec::Length { line: LineRef::of(a), value: 100.0 }),
                rule(RuleSpec::Horizontal { a: AnchorRef::new(a, "start"), b: AnchorRef::new(a, "end") }),
                rule(RuleSpec::Coincident { a: AnchorRef::new(a, "end"), b: AnchorRef::new(b, "start") }),
                rule(RuleSpec::Perpendicular { a: LineRef::of(a), b: LineRef::of(b) }),
            ],
        ),
        ApplyOptions::default(),
    )
    .expect("solve");
    let end = e.evaluate(a).expect("a").anchor("end").expect("end");
    assert!(end.distance(Point::new(100.0, 0.0)) < 1e-6, "{end:?}");
    let corner = e.evaluate(b).expect("b").anchor("start").expect("start");
    assert!(corner.distance(end) < 1e-9);
    let bytes = write_dotl(&DotlFile::new(e.document().clone())).expect("save");
    let (file, _) = read_dotl(&bytes, &DotlLimits::default()).expect("open");
    assert_eq!(file.document.content_hash().expect("hash"), e.document().content_hash().expect("hash"));
    let (svg, _) = export_svg(&mut e, &SvgExportOptions::default());
    assert!(svg.contains("<svg"));
    println!("consumer ok: {} bytes .dotl, {} bytes SVG", bytes.len(), svg.len());
}
`,
)
run('cargo', ['run', '--quiet'], app)
console.log(`\n✓ packaged crates ${version} built and used by ${app}`)

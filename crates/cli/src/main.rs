//! `dotloom` command-line tool: inspect, validate and convert Dotloom files.
//!
//! Exit codes: 0 success, 1 invalid input or failed validation/conversion,
//! 2 usage error, 3 I/O error, 4 capability not available in this build/machine.

use std::{
    io::Write as _,
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::{Parser, Subcommand};
use dotloom_engine::{Engine, EntityTypeDef, document::Document};
use dotloom_io::{
    ConversionReport, DotlFile, DotlLimits,
    dotl::{read_dotl, save_atomic, write_dotl},
    dxf, import_into, svg,
};
use serde_json::json;

#[derive(Parser)]
#[command(name = "dotloom", version, about = "Inspect, validate and convert Dotloom (.dotl), SVG and DXF files")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
    /// Print machine-readable JSON.
    #[arg(long, global = true)]
    json: bool,
    /// Plugin type definitions (JSON object or array); may be repeated.
    #[arg(long = "plugins", global = true)]
    plugins: Vec<PathBuf>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Summarize a file.
    Inspect {
        /// Input (.dotl, .svg, .dxf).
        input: PathBuf,
    },
    /// Validate a .dotl file: container, schema, references and every hard rule.
    Validate {
        /// Input .dotl.
        input: PathBuf,
    },
    /// Convert between .dotl, .svg, .dxf (and .png with the `png` feature).
    Convert {
        /// Input file.
        input: PathBuf,
        /// Output file (format from the extension).
        output: PathBuf,
        /// PNG width in pixels.
        #[arg(long, default_value_t = 1600)]
        width: u32,
        /// PNG background color (`#rrggbb`), transparent when omitted.
        #[arg(long)]
        background: Option<String>,
    },
}

enum Failure {
    Invalid(String, Option<serde_json::Value>),
    Io(String),
    Capability(String),
}

impl Failure {
    fn code(&self) -> u8 {
        match self {
            Self::Invalid(..) => 1,
            Self::Io(_) => 3,
            Self::Capability(_) => 4,
        }
    }
    fn message(&self) -> &str {
        match self {
            Self::Invalid(m, _) | Self::Io(m) | Self::Capability(m) => m,
        }
    }
}

fn ext(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

fn read(p: &Path) -> Result<Vec<u8>, Failure> {
    std::fs::read(p).map_err(|e| Failure::Io(format!("{}: {e}", p.display())))
}

fn engine_with_plugins(paths: &[PathBuf]) -> Result<Engine, Failure> {
    let mut e = Engine::default();
    for p in paths {
        let text =
            String::from_utf8(read(p)?).map_err(|_| Failure::Invalid(format!("{}: not UTF-8", p.display()), None))?;
        let v: serde_json::Value =
            serde_json::from_str(&text).map_err(|x| Failure::Invalid(format!("{}: {x}", p.display()), None))?;
        let defs: Vec<EntityTypeDef> = match v {
            serde_json::Value::Array(_) => serde_json::from_value(v),
            other => serde_json::from_value(other).map(|d| vec![d]),
        }
        .map_err(|x| Failure::Invalid(format!("{}: {x}", p.display()), None))?;
        for d in defs {
            e.register_type(d, &p.display().to_string()).map_err(|x| Failure::Invalid(x.to_string(), None))?;
        }
    }
    Ok(e)
}

/// Load any supported input into an engine.
fn load(input: &Path, plugins: &[PathBuf]) -> Result<(Engine, Option<DotlFile>, Option<ConversionReport>), Failure> {
    let mut engine = engine_with_plugins(plugins)?;
    let bytes = read(input)?;
    match ext(input).as_str() {
        "dotl" => {
            let (file, _) =
                read_dotl(&bytes, &DotlLimits::default()).map_err(|e| Failure::Invalid(e.to_string(), None))?;
            engine.load(file.document.clone()).map_err(|e| Failure::Invalid(e.to_string(), None))?;
            Ok((engine, Some(file), None))
        }
        "svg" => {
            let text = String::from_utf8(bytes).map_err(|_| Failure::Invalid("SVG is not UTF-8".into(), None))?;
            let imp = svg::import_svg(&text).map_err(|e| Failure::Invalid(e.to_string(), None))?;
            let layers: Vec<_> = imp.layers.iter().map(|n| (n.clone(), None)).collect();
            import_into(&mut engine, "Import SVG", &layers, imp.entities)
                .map_err(|e| Failure::Invalid(e.to_string(), None))?;
            Ok((engine, None, Some(imp.report)))
        }
        "dxf" => {
            let imp = dxf::import_dxf(&bytes).map_err(|e| Failure::Invalid(e.to_string(), None))?;
            import_into(&mut engine, "Import DXF", &imp.layers, imp.entities)
                .map_err(|e| Failure::Invalid(e.to_string(), None))?;
            Ok((engine, None, Some(imp.report)))
        }
        other => Err(Failure::Invalid(format!("unsupported input format `.{other}`"), None)),
    }
}

fn summary(doc: &Document) -> serde_json::Value {
    let mut types: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for e in doc.entities() {
        *types.entry(e.type_id.to_string()).or_insert(0) += 1;
    }
    json!({
        "entities": doc.entity_count(),
        "constraints": doc.constraint_count(),
        "layers": doc.layers().iter().map(|l| &l.name).collect::<Vec<_>>(),
        "types": types,
        "hash": doc.content_hash().ok(),
    })
}

fn run(cli: Cli) -> Result<serde_json::Value, Failure> {
    match cli.command {
        Cmd::Inspect { input } => {
            let (mut engine, file, report) = load(&input, &cli.plugins)?;
            let (status, diags) = engine.analyze();
            let mut v = summary(engine.document());
            v["file"] = json!(input.display().to_string());
            v["status"] = serde_json::to_value(status).unwrap_or_default();
            v["diagnostics"] = serde_json::to_value(diags).unwrap_or_default();
            if let Some(f) = &file {
                v["plugins"] =
                    serde_json::to_value(dotloom_io::dotl::plugin_requirements(&f.document)).unwrap_or_default();
                v["assets"] = json!(f.assets.keys().collect::<Vec<_>>());
                v["readOnly"] = json!(
                    f.document
                        .entities()
                        .filter(
                            |e| e.type_id.namespace() != "dotloom" && engine.registry().enabled(&e.type_id).is_none()
                        )
                        .count()
                );
            }
            if let Some(r) = report {
                v["import"] = serde_json::to_value(r).unwrap_or_default();
            }
            let _ = engine.take_events();
            Ok(v)
        }
        Cmd::Validate { input } => {
            if ext(&input) != "dotl" {
                return Err(Failure::Invalid("validate expects a .dotl file".into(), None));
            }
            let (engine, file, _) = load(&input, &cli.plugins)?;
            let violations = engine.verify();
            let missing: Vec<String> = file
                .as_ref()
                .map(|f| dotloom_io::dotl::plugin_requirements(&f.document))
                .unwrap_or_default()
                .into_iter()
                .filter(|p| engine.registry().enabled(&p.type_id).is_none())
                .map(|p| p.type_id.to_string())
                .collect();
            let v = json!({
                "file": input.display().to_string(),
                "valid": violations.is_empty(),
                "violations": violations.iter().map(|(w, r, t)| json!({"rule": w, "residual": r, "tolerance": t})).collect::<Vec<_>>(),
                "missingPlugins": missing,
                "summary": summary(engine.document()),
            });
            if violations.is_empty() {
                Ok(v)
            } else {
                Err(Failure::Invalid(format!("{} hard rule(s) violated", violations.len()), Some(v)))
            }
        }
        Cmd::Convert { input, output, width, background } => {
            let (mut engine, file, import_report) = load(&input, &cli.plugins)?;
            let (bytes, export_report): (Vec<u8>, Option<ConversionReport>) = match ext(&output).as_str() {
                "dotl" => {
                    let mut f = file.unwrap_or_else(|| DotlFile::new(engine.document().clone()));
                    f.document = engine.document().clone();
                    (write_dotl(&f).map_err(|e| Failure::Invalid(e.to_string(), None))?, None)
                }
                "svg" => {
                    let (s, r) = svg::export_svg(&mut engine, &svg::SvgExportOptions::default());
                    (s.into_bytes(), Some(r))
                }
                "dxf" => {
                    let (s, r) = dxf::export_dxf(&mut engine);
                    (s.into_bytes(), Some(r))
                }
                "png" => png(&mut engine, width, background.as_deref())?,
                other => return Err(Failure::Invalid(format!("unsupported output format `.{other}`"), None)),
            };
            save_atomic(&output, &bytes).map_err(|e| Failure::Io(format!("{}: {e}", output.display())))?;
            Ok(json!({
                "input": input.display().to_string(),
                "output": output.display().to_string(),
                "bytes": bytes.len(),
                "import": import_report,
                "export": export_report,
            }))
        }
    }
}

#[cfg(not(feature = "png"))]
fn png(_: &mut Engine, _: u32, _: Option<&str>) -> Result<(Vec<u8>, Option<ConversionReport>), Failure> {
    Err(Failure::Capability("PNG export needs a build with the `png` feature (GPU renderer)".into()))
}

/// Raster export through the wgpu renderer (needs a GPU or software adapter).
#[cfg(feature = "png")]
fn png(
    engine: &mut Engine,
    width: u32,
    background: Option<&str>,
) -> Result<(Vec<u8>, Option<ConversionReport>), Failure> {
    use dotloom_io::{Loss, LossKind};
    use dotloom_render::{Grid, Headless, RendererOptions, Theme, View, color::parse_hex};

    const MARGIN: f64 = 24.0;
    let delta = engine.full_scene();
    let bounds = delta
        .upserts
        .iter()
        .map(|i| i.bbox)
        .filter(|b| !b.is_empty() && b.min.is_finite() && b.max.is_finite())
        .fold(dotloom_engine::geometry::Aabb::EMPTY, dotloom_engine::geometry::Aabb::union);
    if bounds.is_empty() {
        return Err(Failure::Invalid("the document has nothing to draw".into(), None));
    }
    let mut theme = Theme::light();
    theme.background = match background {
        Some(b) => parse_hex(b).ok_or_else(|| Failure::Invalid(format!("invalid --background color `{b}`"), None))?,
        None => 0,
    };
    let width = width.clamp(16, 8192);
    let inner_w = f64::from(width) - 2.0 * MARGIN;
    let aspect = (bounds.height() / bounds.width().max(1e-9)).clamp(1e-3, 1e3);
    let height = (inner_w * aspect + 2.0 * MARGIN).round().clamp(16.0, 8192.0);
    let view = View { center: [0.0, 0.0], scale: 1.0, width: f64::from(width), height, dpr: 1.0 }.fit(bounds, MARGIN);
    let mut h = Headless::new(RendererOptions::default()).map_err(|e| Failure::Capability(e.to_string()))?;
    let adapter = h.adapter().clone();
    let entities = delta.upserts.len();
    let r = h.renderer();
    r.set_theme(theme);
    r.set_grid(Grid { visible: false, ..Grid::default() });
    r.set_view(view).map_err(|e| Failure::Invalid(e.to_string(), None))?;
    r.apply_delta(delta);
    let (img, _) = h.render().map_err(|e| Failure::Capability(e.to_string()))?;
    let bytes = img.to_png().map_err(|e| Failure::Io(e.to_string()))?;
    let report = ConversionReport {
        entities,
        losses: vec![Loss {
            kind: LossKind::Constraints,
            what: "raster image keeps no editable data".into(),
            count: 1,
        }],
        notes: vec![format!(
            "rendered {}x{} px on {} ({}, {})",
            img.width, img.height, adapter.name, adapter.backend, adapter.device_type
        )],
    };
    Ok((bytes, Some(report)))
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            let code = if e.use_stderr() { 2 } else { 0 };
            let _ = e.print();
            return ExitCode::from(code);
        }
    };
    let json_out = cli.json;
    let result = run(cli);
    let mut stdout = std::io::stdout();
    match result {
        Ok(v) => {
            if json_out {
                let _ = writeln!(stdout, "{}", serde_json::to_string_pretty(&v).unwrap_or_default());
            } else {
                let _ = writeln!(stdout, "{}", human(&v));
            }
            ExitCode::SUCCESS
        }
        Err(f) => {
            if json_out {
                let mut v = match &f {
                    Failure::Invalid(_, Some(d)) => d.clone(),
                    _ => json!({}),
                };
                v["error"] = json!({"code": f.code(), "message": f.message()});
                let _ = writeln!(stdout, "{}", serde_json::to_string_pretty(&v).unwrap_or_default());
            } else {
                eprintln!("error: {}", f.message());
            }
            ExitCode::from(f.code())
        }
    }
}

fn human(v: &serde_json::Value) -> String {
    let mut lines = Vec::new();
    if let Some(o) = v.as_object() {
        for (k, val) in o {
            match val {
                serde_json::Value::Null => {}
                serde_json::Value::String(s) => lines.push(format!("{k}: {s}")),
                other => lines.push(format!("{k}: {}", other)),
            }
        }
    }
    lines.join("\n")
}

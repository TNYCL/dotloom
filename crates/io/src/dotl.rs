//! `.dotl` project files (ADR-0005).

use std::collections::{BTreeMap, BTreeSet};

use dotloom_document::{Document, Limits, MigrationNote, PropValue, SCHEMA_VERSION, TypeId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::zip::{Archive, ZipError, ZipLimits, write};

/// Container format version written by this library.
pub const FORMAT_VERSION: u32 = 1;
/// MIME type.
pub const MEDIA_TYPE: &str = "application/vnd.dotloom.project+zip";
/// Prefix of text properties that reference an asset: `asset:assets/<sha256>.png`.
pub const ASSET_PREFIX: &str = "asset:";

/// A plugin type used by the document.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginRequirement {
    /// Type ID.
    pub type_id: TypeId,
    /// Highest type version used.
    pub version: u32,
}

/// Asset metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetInfo {
    /// Path inside the container (`assets/<sha256>.<ext>`).
    pub path: String,
    /// Media type.
    pub media_type: String,
    /// Size in bytes.
    pub size: u64,
    /// Hex SHA-256 of the content.
    pub sha256: String,
}

/// Who wrote the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Producer {
    /// Name.
    pub name: String,
    /// Version.
    pub version: String,
}

/// `manifest.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    /// Always `"dotloom"`.
    pub format: String,
    /// Container version.
    pub format_version: u32,
    /// Document schema version of `document.json`.
    pub schema_version: u32,
    /// Producer.
    pub producer: Producer,
    /// Plugin types required to edit every entity.
    #[serde(default)]
    pub plugins: Vec<PluginRequirement>,
    /// Assets.
    #[serde(default)]
    pub assets: Vec<AssetInfo>,
    /// Unknown fields preserved.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// An in-memory `.dotl` project.
#[derive(Debug, Clone, PartialEq)]
pub struct DotlFile {
    /// The document.
    pub document: Document,
    /// Optional view state (camera, panels); never geometry.
    pub view: Option<Value>,
    /// Assets by container path.
    pub assets: BTreeMap<String, Vec<u8>>,
    /// Entries this version does not understand, preserved on save.
    pub unknown_entries: BTreeMap<String, Vec<u8>>,
    /// Unknown manifest fields, preserved on save.
    pub manifest_extra: BTreeMap<String, Value>,
}

impl DotlFile {
    /// Wrap a document.
    #[must_use]
    pub fn new(document: Document) -> Self {
        Self {
            document,
            view: None,
            assets: BTreeMap::new(),
            unknown_entries: BTreeMap::new(),
            manifest_extra: BTreeMap::new(),
        }
    }
}

/// Load report.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LoadReport {
    /// Schema migrations applied.
    pub migrations: Vec<MigrationNote>,
    /// Plugins required by the file.
    pub plugins: Vec<PluginRequirement>,
    /// Warnings (preserved unknown entries, ...).
    pub warnings: Vec<String>,
}

/// `.dotl` errors.
#[derive(Debug, Clone, PartialEq, Error)]
#[non_exhaustive]
pub enum DotlError {
    /// Container problem.
    #[error(transparent)]
    Zip(#[from] ZipError),
    /// Required entry missing.
    #[error("missing `{0}` in the container")]
    MissingEntry(&'static str),
    /// Manifest problem.
    #[error("invalid manifest: {0}")]
    Manifest(String),
    /// Container written by a newer, incompatible Dotloom.
    #[error("file format version {found} is newer than supported version {supported}")]
    FutureFormat {
        /// Found.
        found: u32,
        /// Supported.
        supported: u32,
    },
    /// Document problem.
    #[error("invalid document: {0}")]
    Document(String),
    /// Asset referenced but missing or corrupt.
    #[error("asset `{0}`: {1}")]
    Asset(String, String),
}

/// Reader limits.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DotlLimits {
    /// Container limits.
    pub zip: ZipLimits,
    /// Document limits.
    pub document: Limits,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// SHA-256 hex digest.
#[must_use]
pub fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

/// Asset paths referenced by the document (`asset:` text properties).
#[must_use]
pub fn referenced_assets(doc: &Document) -> BTreeSet<String> {
    doc.entities()
        .flat_map(|e| e.props.values())
        .filter_map(|v| match v {
            PropValue::Text(t) => t.strip_prefix(ASSET_PREFIX).map(ToOwned::to_owned),
            _ => None,
        })
        .collect()
}

/// Plugin requirements of a document.
#[must_use]
pub fn plugin_requirements(doc: &Document) -> Vec<PluginRequirement> {
    let mut m: BTreeMap<TypeId, u32> = BTreeMap::new();
    for e in doc.entities() {
        if e.type_id.namespace() != "dotloom" {
            let v = m.entry(e.type_id.clone()).or_insert(0);
            *v = (*v).max(e.type_version);
        }
    }
    m.into_iter().map(|(type_id, version)| PluginRequirement { type_id, version }).collect()
}

fn media_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "svg" => "image/svg+xml",
        "ttf" => "font/ttf",
        "woff2" => "font/woff2",
        "json" => "application/json",
        _ => "application/octet-stream",
    }
}

/// Store an asset and return its container path (content addressed).
pub fn add_asset(file: &mut DotlFile, ext: &str, data: Vec<u8>) -> String {
    let ext: String = ext.chars().filter(char::is_ascii_alphanumeric).take(8).collect::<String>().to_ascii_lowercase();
    let path = format!("assets/{}.{}", sha256_hex(&data), if ext.is_empty() { "bin" } else { &ext });
    file.assets.insert(path.clone(), data);
    path
}

/// Serialize a project to `.dotl` bytes (deterministic for equal input).
pub fn write_dotl(file: &DotlFile) -> Result<Vec<u8>, DotlError> {
    let doc_json = serde_json::to_vec_pretty(&file.document).map_err(|e| DotlError::Document(e.to_string()))?;
    let referenced = referenced_assets(&file.document);
    for r in &referenced {
        if !file.assets.contains_key(r) {
            return Err(DotlError::Asset(r.clone(), "referenced but missing".into()));
        }
    }
    let assets: Vec<AssetInfo> = file
        .assets
        .iter()
        .map(|(p, d)| AssetInfo {
            path: p.clone(),
            media_type: media_type(p).into(),
            size: d.len() as u64,
            sha256: sha256_hex(d),
        })
        .collect();
    let manifest = Manifest {
        format: "dotloom".into(),
        format_version: FORMAT_VERSION,
        schema_version: SCHEMA_VERSION,
        producer: Producer { name: "dotloom".into(), version: env!("CARGO_PKG_VERSION").into() },
        plugins: plugin_requirements(&file.document),
        assets,
        extra: file.manifest_extra.clone(),
    };
    let mut entries = vec![
        (
            "manifest.json".to_owned(),
            serde_json::to_vec_pretty(&manifest).map_err(|e| DotlError::Manifest(e.to_string()))?,
        ),
        ("document.json".to_owned(), doc_json),
    ];
    if let Some(v) = &file.view {
        entries
            .push(("view.json".into(), serde_json::to_vec_pretty(v).map_err(|e| DotlError::Manifest(e.to_string()))?));
    }
    for (p, d) in &file.assets {
        entries.push((p.clone(), d.clone()));
    }
    for (p, d) in &file.unknown_entries {
        entries.push((p.clone(), d.clone()));
    }
    Ok(write(&entries)?)
}

/// Parse `.dotl` bytes: bounded container parsing, manifest checks, schema
/// migration, document validation and asset verification.
pub fn read_dotl(bytes: &[u8], limits: &DotlLimits) -> Result<(DotlFile, LoadReport), DotlError> {
    let archive = Archive::parse(bytes, limits.zip)?;
    let get = |name: &'static str| -> Result<Vec<u8>, DotlError> {
        let e = archive.entry(name).ok_or(DotlError::MissingEntry(name))?;
        Ok(archive.read(e)?)
    };
    let manifest: Manifest =
        serde_json::from_slice(&get("manifest.json")?).map_err(|e| DotlError::Manifest(e.to_string()))?;
    if manifest.format != "dotloom" {
        return Err(DotlError::Manifest(format!("format is `{}`, expected `dotloom`", manifest.format)));
    }
    if manifest.format_version > FORMAT_VERSION {
        return Err(DotlError::FutureFormat { found: manifest.format_version, supported: FORMAT_VERSION });
    }
    let doc_text = get("document.json")?;
    let (document, migrations) =
        Document::from_json_slice(&doc_text, &limits.document).map_err(|e| DotlError::Document(e.to_string()))?;
    let view = match archive.entry("view.json") {
        Some(e) => Some(
            serde_json::from_slice(&archive.read(e)?).map_err(|x| DotlError::Manifest(format!("view.json: {x}")))?,
        ),
        None => None,
    };
    let mut assets = BTreeMap::new();
    for a in &manifest.assets {
        let e = archive
            .entry(&a.path)
            .ok_or_else(|| DotlError::Asset(a.path.clone(), "listed in the manifest but missing".into()))?;
        let data = archive.read(e)?;
        if sha256_hex(&data) != a.sha256 {
            return Err(DotlError::Asset(a.path.clone(), "checksum mismatch".into()));
        }
        assets.insert(a.path.clone(), data);
    }
    for r in referenced_assets(&document) {
        if !assets.contains_key(&r) {
            return Err(DotlError::Asset(r, "referenced by the document but missing".into()));
        }
    }
    let known: BTreeSet<&str> = ["manifest.json", "document.json", "view.json"].into_iter().collect();
    let mut unknown_entries = BTreeMap::new();
    let mut warnings = Vec::new();
    for e in archive.entries() {
        if known.contains(e.name.as_str()) || assets.contains_key(&e.name) || e.name.ends_with('/') {
            continue;
        }
        warnings.push(format!("preserved unknown entry `{}`", e.name));
        unknown_entries.insert(e.name.clone(), archive.read(e)?);
    }
    let report = LoadReport { migrations, plugins: manifest.plugins.clone(), warnings };
    Ok((DotlFile { document, view, assets, unknown_entries, manifest_extra: manifest.extra }, report))
}

/// Write bytes to `path` atomically: a temporary file in the same directory is
/// written and flushed, then renamed over the target. On failure the existing file
/// is left untouched and the temporary file is removed.
#[cfg(not(target_arch = "wasm32"))]
pub fn save_atomic(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let dir = match path.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
        _ => std::path::PathBuf::from("."),
    };
    let name = path.file_name().map_or_else(|| "dotloom".into(), |n| n.to_string_lossy().into_owned());
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let result = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

//! Entity type definitions and the type registry (ADR-0006).
//!
//! A plugin entity type is *data*: a JSON-serializable [`EntityTypeDef`] with a
//! typed property schema, derived values, anchors, drawable primitive recipes,
//! constraint templates and property migrations, all written in the expression
//! language of [`crate::lang`]. Definitions are compiled (parsed, dimension-checked)
//! at registration; invalid definitions are rejected with typed errors.

use std::{collections::BTreeMap, sync::Arc};

use dotloom_document::{Cmp, Color, StrengthSpec, TypeId, builtin::types};
use dotloom_geometry::{
    AnchorKind, HAlign, VAlign,
    units::{Dim, Quantity},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::lang::{Binding, Compiled, LangError, Scope, Ty, compile, compile_as};

/// Registry errors.
#[derive(Debug, Clone, PartialEq, Error)]
#[non_exhaustive]
pub enum RegistryError {
    /// The type ID is already registered.
    #[error("type `{0}` is already registered")]
    Duplicate(String),
    /// Built-in types cannot be redefined.
    #[error("type `{0}` is reserved by Dotloom")]
    Reserved(String),
    /// The type is not registered.
    #[error("type `{0}` is not registered")]
    Unknown(String),
    /// The definition is invalid.
    #[error("invalid definition of `{type_id}` at {location}: {error}")]
    Invalid {
        /// Type.
        type_id: String,
        /// Where in the definition.
        location: String,
        /// What is wrong.
        error: String,
    },
}

/// Physical dimension names used in definitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DimName {
    /// Length (mm).
    #[default]
    Length,
    /// Angle (rad).
    Angle,
    /// Time (s).
    Time,
    /// Dimensionless.
    Scalar,
}

impl DimName {
    /// As [`Dim`].
    #[must_use]
    pub const fn dim(self) -> Dim {
        match self {
            Self::Length => Dim::LENGTH,
            Self::Angle => Dim::ANGLE,
            Self::Time => Dim::TIME,
            Self::Scalar => Dim::SCALAR,
        }
    }
}

/// A number literal in a definition: a plain number (canonical units) or a string
/// with a unit such as `"60cm"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NumberLit {
    /// Canonical value.
    Plain(f64),
    /// Value with unit.
    WithUnit(String),
}

impl NumberLit {
    /// Canonical value, checking the dimension.
    pub fn value(&self, dim: Dim) -> Result<f64, String> {
        match self {
            Self::Plain(v) if v.is_finite() => Ok(*v),
            Self::Plain(_) => Err("not finite".into()),
            Self::WithUnit(s) => {
                let q = Quantity::parse(s).map_err(|e| e.to_string())?;
                if q.dim == dim || q.dim == Dim::SCALAR {
                    Ok(q.value)
                } else {
                    Err(format!("`{s}` has dimension {} but {dim} is required", q.dim))
                }
            }
        }
    }
}

/// What happens to a referencing entity when its target is deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OnDelete {
    /// Delete the referencing entity too (door with its wall).
    #[default]
    Cascade,
    /// Remove the reference property.
    Clear,
    /// Refuse the deletion.
    Reject,
}

fn yes() -> bool {
    true
}

/// How strongly a solver property keeps its previous value when rules force
/// changes (relative stay weight).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StayLevel {
    /// Changes first (positions such as a door offset).
    Low,
    /// Default.
    #[default]
    Normal,
    /// Changes last (sizes such as a door width).
    High,
}

impl StayLevel {
    /// Stay multiplier.
    #[must_use]
    pub const fn factor(self) -> f64 {
        match self {
            Self::Low => 0.2,
            Self::Normal => 1.0,
            Self::High => 5.0,
        }
    }
}

/// Property schema.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum PropDef {
    /// Number with a dimension.
    Number {
        /// Dimension.
        #[serde(default)]
        dim: DimName,
        /// Default value.
        #[serde(default)]
        default: Option<NumberLit>,
        /// Minimum (enforced as a hard rule).
        #[serde(default)]
        min: Option<NumberLit>,
        /// Maximum (enforced as a hard rule).
        #[serde(default)]
        max: Option<NumberLit>,
        /// Whether the solver may change this property.
        #[serde(default = "yes")]
        solve: bool,
        /// Relative stay weight.
        #[serde(default)]
        stay: StayLevel,
        /// Label.
        #[serde(default)]
        label: Option<String>,
    },
    /// Point (two length components, solver variables).
    Point {
        /// Default.
        #[serde(default)]
        default: Option<[f64; 2]>,
        /// Relative stay weight.
        #[serde(default)]
        stay: StayLevel,
        /// Label.
        #[serde(default)]
        label: Option<String>,
    },
    /// Boolean.
    Bool {
        /// Default.
        #[serde(default)]
        default: Option<bool>,
        /// Label.
        #[serde(default)]
        label: Option<String>,
    },
    /// Free text.
    Text {
        /// Default.
        #[serde(default)]
        default: Option<String>,
        /// Maximum length in characters.
        #[serde(default)]
        max_len: Option<usize>,
        /// Label.
        #[serde(default)]
        label: Option<String>,
    },
    /// One of a fixed set of strings.
    Enum {
        /// Allowed values.
        values: Vec<String>,
        /// Default.
        #[serde(default)]
        default: Option<String>,
        /// Label.
        #[serde(default)]
        label: Option<String>,
    },
    /// Reference to another entity.
    Ref {
        /// Required target type (enables typed member access in expressions).
        #[serde(default)]
        target: Option<TypeId>,
        /// Deletion policy.
        #[serde(default)]
        on_delete: OnDelete,
        /// Whether the reference must be set.
        #[serde(default)]
        required: bool,
        /// Label.
        #[serde(default)]
        label: Option<String>,
    },
}

/// A named expression.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedExpr {
    /// Name.
    pub name: String,
    /// Expression.
    pub expr: String,
}

/// An anchor definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnchorDef {
    /// Anchor name.
    pub name: String,
    /// Point expression (local coordinates).
    pub expr: String,
    /// Snap kind.
    #[serde(default = "default_anchor_kind")]
    pub kind: AnchorKind,
}

fn default_anchor_kind() -> AnchorKind {
    AnchorKind::Vertex
}

/// Style of a primitive recipe.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrimStyle {
    /// Stroke color (default: entity/layer/theme).
    #[serde(default)]
    pub stroke: Option<Color>,
    /// Fill color (regions only).
    #[serde(default)]
    pub fill: Option<Color>,
    /// Stroke width (CSS px).
    #[serde(default)]
    pub width: Option<f64>,
    /// Dash (CSS px).
    #[serde(default)]
    pub dash: Option<Vec<f64>>,
    /// No stroke at all.
    #[serde(default)]
    pub no_stroke: bool,
}

/// Drawable primitive recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum PrimitiveDef {
    /// Segment.
    Line {
        /// Start.
        from: String,
        /// End.
        to: String,
        /// Style.
        #[serde(default)]
        style: PrimStyle,
    },
    /// Polyline.
    Polyline {
        /// Points.
        points: Vec<String>,
        /// Closed.
        #[serde(default)]
        closed: bool,
        /// Style.
        #[serde(default)]
        style: PrimStyle,
    },
    /// Closed polygon (fillable).
    Polygon {
        /// Points.
        points: Vec<String>,
        /// Style.
        #[serde(default)]
        style: PrimStyle,
    },
    /// Circle.
    Circle {
        /// Center.
        center: String,
        /// Radius.
        radius: String,
        /// Style.
        #[serde(default)]
        style: PrimStyle,
    },
    /// Arc.
    Arc {
        /// Center.
        center: String,
        /// Radius.
        radius: String,
        /// Start angle.
        start: String,
        /// Sweep angle.
        sweep: String,
        /// Style.
        #[serde(default)]
        style: PrimStyle,
    },
    /// Text label; `content` may contain `{prop}` placeholders.
    Text {
        /// Anchor point.
        position: String,
        /// Content template.
        content: String,
        /// Height.
        height: String,
        /// Rotation.
        #[serde(default)]
        rotation: Option<String>,
        /// Horizontal alignment.
        #[serde(default)]
        halign: HAlign,
        /// Vertical alignment.
        #[serde(default)]
        valign: VAlign,
        /// Style (stroke color is the text color).
        #[serde(default)]
        style: PrimStyle,
    },
}

/// Constraint template instantiated for every entity of the type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TemplateDef {
    /// Left expression.
    pub lhs: String,
    /// Operator.
    pub op: Cmp,
    /// Right expression.
    pub rhs: String,
    /// Label for diagnostics.
    #[serde(default)]
    pub label: Option<String>,
    /// Strength.
    #[serde(default)]
    pub strength: StrengthSpec,
}

/// Circle-like interpretation for circle rules (concentric, tangency, radius).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CircleDef {
    /// Center expression.
    pub center: String,
    /// Radius expression.
    pub radius: String,
}

/// Property migration from `from` to `from + 1`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MigrationDef {
    /// Source version.
    pub from: u32,
    /// Renamed properties `old → new`.
    #[serde(default)]
    pub rename: BTreeMap<String, String>,
    /// Properties set when missing (literal JSON values).
    #[serde(default)]
    pub set: BTreeMap<String, Value>,
    /// Removed properties.
    #[serde(default)]
    pub remove: Vec<String>,
}

fn one() -> u32 {
    1
}

/// A plugin entity type definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityTypeDef {
    /// Namespaced type ID (not `dotloom.*`).
    pub type_id: TypeId,
    /// Schema version of the properties.
    #[serde(default = "one")]
    pub version: u32,
    /// Display label.
    #[serde(default)]
    pub label: String,
    /// Property schema.
    #[serde(default)]
    pub props: BTreeMap<String, PropDef>,
    /// Named helper values, in dependency order.
    #[serde(default)]
    pub derived: Vec<NamedExpr>,
    /// Anchors, in dependency order.
    #[serde(default)]
    pub anchors: Vec<AnchorDef>,
    /// Drawable primitives.
    #[serde(default)]
    pub primitives: Vec<PrimitiveDef>,
    /// Constraint templates.
    #[serde(default)]
    pub constraints: Vec<TemplateDef>,
    /// Circle interpretation.
    #[serde(default)]
    pub circle: Option<CircleDef>,
    /// Property migrations.
    #[serde(default)]
    pub migrations: Vec<MigrationDef>,
}

/// Compiled primitive recipe.
#[derive(Debug, Clone, PartialEq)]
pub enum CompiledPrim {
    /// Segment.
    Line(Compiled, Compiled, PrimStyle),
    /// Polyline.
    Polyline(Vec<Compiled>, bool, PrimStyle),
    /// Polygon.
    Polygon(Vec<Compiled>, PrimStyle),
    /// Circle.
    Circle(Compiled, Compiled, PrimStyle),
    /// Arc.
    Arc(Compiled, Compiled, Compiled, Compiled, PrimStyle),
    /// Text.
    Text {
        /// Position.
        position: Compiled,
        /// Template.
        content: String,
        /// Height.
        height: Compiled,
        /// Rotation.
        rotation: Option<Compiled>,
        /// Alignment.
        halign: HAlign,
        /// Alignment.
        valign: VAlign,
        /// Style.
        style: PrimStyle,
    },
}

/// A compiled constraint template.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledTemplate {
    /// Source definition.
    pub def: TemplateDef,
    /// `lhs − rhs` (scalar) and its dimension.
    pub diff: Compiled,
    /// Dimension of both sides.
    pub dim: Dim,
}

/// Solver parameter of a plugin entity.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamInfo {
    /// Name (`width`, `start.x`).
    pub name: String,
    /// Dimension.
    pub dim: Dim,
    /// Stay multiplier.
    pub stay: f64,
}

/// A registered, compiled plugin type.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledType {
    /// Definition.
    pub def: EntityTypeDef,
    /// Anchors.
    pub anchors: Vec<(String, AnchorKind, Compiled)>,
    /// Primitives.
    pub primitives: Vec<CompiledPrim>,
    /// Templates.
    pub templates: Vec<CompiledTemplate>,
    /// Circle interpretation.
    pub circle: Option<(Compiled, Compiled)>,
    /// Solver parameters in stable order.
    pub params: Vec<ParamInfo>,
    /// Members visible to referencing types.
    pub members: BTreeMap<String, Ty>,
    /// Expression scope of the type (for expression constraints on its entities).
    pub scope: Scope,
}

/// Registry entry.
#[derive(Debug, Clone)]
pub struct TypeEntry {
    /// Compiled type.
    pub compiled: Arc<CompiledType>,
    /// Enabled flag (disabled types make their entities read-only).
    pub enabled: bool,
    /// Plugin that registered the type.
    pub plugin: String,
}

/// Members of built-in types usable through typed references.
#[must_use]
pub fn builtin_members(t: &TypeId) -> Option<BTreeMap<String, Ty>> {
    let (anchors, lengths): (&[&str], &[&str]) = match t.as_str() {
        types::POINT => (&["point"], &[]),
        types::LINE => (&["start", "end", "mid"], &[]),
        types::POLYLINE | types::PATH => (&["start", "end"], &[]),
        types::RECT => (&["c0", "c1", "c2", "c3", "e0", "e1", "e2", "e3", "center"], &["width", "height"]),
        types::CIRCLE => (&["center", "q0", "q1", "q2", "q3"], &["r"]),
        types::ARC => (&["start", "end", "mid", "center"], &["r"]),
        types::TEXT => (&["insert"], &[]),
        _ => return None,
    };
    let mut m: BTreeMap<String, Ty> = anchors.iter().map(|n| ((*n).to_owned(), Ty::Vector(Dim::LENGTH))).collect();
    m.extend(lengths.iter().map(|n| ((*n).to_owned(), Ty::Scalar(Dim::LENGTH))));
    Some(m)
}

/// The type registry.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    types: BTreeMap<TypeId, TypeEntry>,
}

fn invalid(t: &TypeId, location: impl Into<String>, e: impl ToString) -> RegistryError {
    RegistryError::Invalid { type_id: t.to_string(), location: location.into(), error: e.to_string() }
}

impl Registry {
    /// Empty registry (built-in types need no registration).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registered plugin type.
    #[must_use]
    pub fn get(&self, t: &TypeId) -> Option<&TypeEntry> {
        self.types.get(t)
    }

    /// Enabled plugin type.
    #[must_use]
    pub fn enabled(&self, t: &TypeId) -> Option<&Arc<CompiledType>> {
        self.types.get(t).filter(|e| e.enabled).map(|e| &e.compiled)
    }

    /// All registered types.
    pub fn types(&self) -> impl Iterator<Item = (&TypeId, &TypeEntry)> {
        self.types.iter()
    }

    /// Members of any type (built-in or registered).
    #[must_use]
    pub fn members_of(&self, t: &TypeId) -> Option<BTreeMap<String, Ty>> {
        builtin_members(t).or_else(|| self.types.get(t).map(|e| e.compiled.members.clone()))
    }

    /// Register a definition. Fails on duplicates, reserved namespaces and invalid
    /// definitions (nothing is registered in that case).
    pub fn register(&mut self, def: EntityTypeDef, plugin: impl Into<String>) -> Result<(), RegistryError> {
        let t = def.type_id.clone();
        if t.namespace() == "dotloom" {
            return Err(RegistryError::Reserved(t.to_string()));
        }
        if self.types.contains_key(&t) {
            return Err(RegistryError::Duplicate(t.to_string()));
        }
        let compiled = self.compile(def)?;
        self.types.insert(t, TypeEntry { compiled: Arc::new(compiled), enabled: true, plugin: plugin.into() });
        Ok(())
    }

    /// Remove a type definition.
    pub fn unregister(&mut self, t: &TypeId) -> Result<TypeEntry, RegistryError> {
        self.types.remove(t).ok_or_else(|| RegistryError::Unknown(t.to_string()))
    }

    /// Enable or disable a type.
    pub fn set_enabled(&mut self, t: &TypeId, enabled: bool) -> Result<(), RegistryError> {
        let e = self.types.get_mut(t).ok_or_else(|| RegistryError::Unknown(t.to_string()))?;
        e.enabled = enabled;
        Ok(())
    }

    fn compile(&self, def: EntityTypeDef) -> Result<CompiledType, RegistryError> {
        let t = def.type_id.clone();
        if def.version == 0 {
            return Err(invalid(&t, "version", "must be ≥ 1"));
        }
        let mut scope = Scope { names: BTreeMap::new(), has_axis: true };
        let mut params = Vec::new();
        let reserved = ["pi", "axis", "self"];
        for (name, p) in &def.props {
            if reserved.contains(&name.as_str()) || !valid_name(name) {
                return Err(invalid(&t, format!("props.{name}"), "invalid property name"));
            }
            match p {
                PropDef::Number { dim, default, min, max, solve, stay, .. } => {
                    for (what, lit) in [("default", default), ("min", min), ("max", max)] {
                        if let Some(l) = lit {
                            l.value(dim.dim()).map_err(|e| invalid(&t, format!("props.{name}.{what}"), e))?;
                        }
                    }
                    scope.names.insert(name.clone(), Binding::NumberProp(dim.dim()));
                    if *solve {
                        params.push(ParamInfo { name: name.clone(), dim: dim.dim(), stay: stay.factor() });
                    }
                }
                PropDef::Point { stay, .. } => {
                    scope.names.insert(name.clone(), Binding::PointProp);
                    params.push(ParamInfo { name: format!("{name}.x"), dim: Dim::LENGTH, stay: stay.factor() });
                    params.push(ParamInfo { name: format!("{name}.y"), dim: Dim::LENGTH, stay: stay.factor() });
                }
                PropDef::Ref { target, .. } => {
                    let members = match target {
                        Some(tt) if *tt == t => Some(BTreeMap::new()),
                        Some(tt) => Some(self.members_of(tt).ok_or_else(|| {
                            invalid(&t, format!("props.{name}.target"), format!("type `{tt}` must be registered first"))
                        })?),
                        None => None,
                    };
                    scope.names.insert(name.clone(), Binding::RefProp(members));
                }
                PropDef::Enum { values, default, .. } => {
                    if values.is_empty() || default.as_ref().is_some_and(|d| !values.contains(d)) {
                        return Err(invalid(&t, format!("props.{name}"), "enum needs values and a valid default"));
                    }
                }
                PropDef::Bool { .. } | PropDef::Text { .. } => {}
            }
        }
        let err = |loc: String| {
            let t = t.clone();
            move |e: LangError| invalid(&t, loc, e)
        };
        for d in &def.derived {
            if scope.names.contains_key(&d.name) || !valid_name(&d.name) {
                return Err(invalid(&t, format!("derived.{}", d.name), "name is already defined or invalid"));
            }
            let c = compile(&d.expr, &scope).map_err(err(format!("derived.{}", d.name)))?;
            scope.names.insert(d.name.clone(), Binding::Value(c));
        }
        let mut anchors = Vec::new();
        let mut members: BTreeMap<String, Ty> = BTreeMap::new();
        for a in &def.anchors {
            if !valid_name(&a.name) {
                return Err(invalid(&t, format!("anchors.{}", a.name), "invalid anchor name"));
            }
            let c = compile_as(&a.expr, &scope, Ty::Vector(Dim::LENGTH)).map_err(err(format!("anchors.{}", a.name)))?;
            members.insert(a.name.clone(), Ty::Vector(Dim::LENGTH));
            if !scope.names.contains_key(&a.name) {
                scope.names.insert(a.name.clone(), Binding::Value(c.clone()));
            }
            anchors.push((a.name.clone(), a.kind, c));
        }
        for p in &params {
            if !p.name.contains('.') {
                members.entry(p.name.clone()).or_insert(Ty::Scalar(p.dim));
            }
        }
        let pt = |src: &str, loc: String| compile_as(src, &scope, Ty::Vector(Dim::LENGTH)).map_err(err(loc));
        let len = |src: &str, loc: String| compile_as(src, &scope, Ty::Scalar(Dim::LENGTH)).map_err(err(loc));
        let ang = |src: &str, loc: String| compile_as(src, &scope, Ty::Scalar(Dim::ANGLE)).map_err(err(loc));
        let mut primitives = Vec::new();
        for (i, p) in def.primitives.iter().enumerate() {
            let loc = |f: &str| format!("primitives[{i}].{f}");
            primitives.push(match p {
                PrimitiveDef::Line { from, to, style } => {
                    CompiledPrim::Line(pt(from, loc("from"))?, pt(to, loc("to"))?, style.clone())
                }
                PrimitiveDef::Polyline { points, closed, style } => CompiledPrim::Polyline(
                    points
                        .iter()
                        .enumerate()
                        .map(|(k, s)| pt(s, loc(&format!("points[{k}]"))))
                        .collect::<Result<_, _>>()?,
                    *closed,
                    style.clone(),
                ),
                PrimitiveDef::Polygon { points, style } => {
                    if points.len() < 3 {
                        return Err(invalid(&t, loc("points"), "polygon needs at least 3 points"));
                    }
                    CompiledPrim::Polygon(
                        points
                            .iter()
                            .enumerate()
                            .map(|(k, s)| pt(s, loc(&format!("points[{k}]"))))
                            .collect::<Result<_, _>>()?,
                        style.clone(),
                    )
                }
                PrimitiveDef::Circle { center, radius, style } => {
                    CompiledPrim::Circle(pt(center, loc("center"))?, len(radius, loc("radius"))?, style.clone())
                }
                PrimitiveDef::Arc { center, radius, start, sweep, style } => CompiledPrim::Arc(
                    pt(center, loc("center"))?,
                    len(radius, loc("radius"))?,
                    ang(start, loc("start"))?,
                    ang(sweep, loc("sweep"))?,
                    style.clone(),
                ),
                PrimitiveDef::Text { position, content, height, rotation, halign, valign, style } => {
                    for name in placeholders(content) {
                        if !def.props.contains_key(&name) {
                            return Err(invalid(&t, loc("content"), format!("unknown placeholder {{{name}}}")));
                        }
                    }
                    CompiledPrim::Text {
                        position: pt(position, loc("position"))?,
                        content: content.clone(),
                        height: len(height, loc("height"))?,
                        rotation: rotation.as_ref().map(|r| ang(r, loc("rotation"))).transpose()?,
                        halign: *halign,
                        valign: *valign,
                        style: style.clone(),
                    }
                }
            });
        }
        let mut templates = Vec::new();
        for (i, tpl) in def.constraints.iter().enumerate() {
            let loc = format!("constraints[{i}]");
            let l = compile(&tpl.lhs, &scope).map_err(err(loc.clone()))?;
            let r = compile(&tpl.rhs, &scope).map_err(err(loc.clone()))?;
            let (Ty::Scalar(dl), Ty::Scalar(dr)) = (l.ty, r.ty) else {
                return Err(invalid(&t, loc, "constraint sides must be scalars"));
            };
            if dl != dr {
                return Err(invalid(&t, loc, format!("cannot compare {dl} with {dr}")));
            }
            let diff = compile(&format!("({}) - ({})", tpl.lhs, tpl.rhs), &scope).map_err(err(loc))?;
            templates.push(CompiledTemplate { def: tpl.clone(), diff, dim: dl });
        }
        // Property bounds become templates too.
        for (name, p) in &def.props {
            if let PropDef::Number { dim, min, max, .. } = p {
                for (lit, op) in [(min, Cmp::Ge), (max, Cmp::Le)] {
                    if let Some(l) = lit {
                        let v = l.value(dim.dim()).map_err(|e| invalid(&t, format!("props.{name}"), e))?;
                        let tpl = TemplateDef {
                            lhs: name.clone(),
                            op,
                            rhs: format!("{v}"),
                            label: Some(format!("{name} {} {v}", if op == Cmp::Ge { "≥" } else { "≤" })),
                            strength: StrengthSpec::Required,
                        };
                        let mut diff = compile(name, &scope).map_err(err(format!("props.{name}")))?;
                        diff.parts = diff
                            .parts
                            .into_iter()
                            .map(|e| dotloom_constraints::Expr::sub(e, dotloom_constraints::Expr::c(v)))
                            .collect();
                        templates.push(CompiledTemplate { def: tpl, diff, dim: dim.dim() });
                    }
                }
            }
        }
        let circle = match &def.circle {
            Some(c) => Some((pt(&c.center, "circle.center".into())?, len(&c.radius, "circle.radius".into())?)),
            None => None,
        };
        let mut seen = std::collections::BTreeSet::new();
        for m in &def.migrations {
            if m.from == 0 || m.from >= def.version || !seen.insert(m.from) {
                return Err(invalid(
                    &t,
                    format!("migrations.from={}", m.from),
                    "migration steps must be unique and below the current version",
                ));
            }
        }
        Ok(CompiledType { def, anchors, primitives, templates, circle, params, members, scope })
    }
}

fn valid_name(n: &str) -> bool {
    let mut c = n.chars();
    c.next().is_some_and(|f| f.is_ascii_alphabetic() || f == '_')
        && c.all(|x| x.is_ascii_alphanumeric() || x == '_')
        && n.len() <= 64
}

/// `{name}` placeholders of a text template.
#[must_use]
pub fn placeholders(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(i) = rest.find('{') {
        let after = &rest[i + 1..];
        match after.find('}') {
            Some(j) => {
                out.push(after[..j].to_owned());
                rest = &after[j + 1..];
            }
            None => break,
        }
    }
    out
}

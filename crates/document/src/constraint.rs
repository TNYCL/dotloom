//! Document-level constraint descriptions.
//!
//! These are data: what the user (or a plugin template) asked for. The engine
//! compiles them into solver rows. Every reference is explicit so validation can
//! reject dangling references before commit.

use std::collections::BTreeMap;

use dotloom_geometry::Point;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{AnchorRef, ConstraintId, EntityId};

/// Which slot of an entity a numeric parameter lives in.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ParamSlot {
    /// A numeric property (`props.width`).
    Prop(String),
    /// A geometry parameter of a built-in shape (`radius`, `a.x`, `width`, ...).
    Geom(String),
}

/// Reference to a numeric parameter: `{"entity": 3, "prop": "width"}` or
/// `{"entity": 3, "geom": "radius"}`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ParamRef {
    /// Entity.
    pub entity: EntityId,
    /// Slot.
    #[serde(flatten)]
    pub slot: ParamSlot,
}

impl ParamRef {
    /// Property parameter.
    #[must_use]
    pub fn prop(entity: EntityId, name: impl Into<String>) -> Self {
        Self { entity, slot: ParamSlot::Prop(name.into()) }
    }

    /// Geometry parameter.
    #[must_use]
    pub fn geom(entity: EntityId, name: impl Into<String>) -> Self {
        Self { entity, slot: ParamSlot::Geom(name.into()) }
    }
}

/// A line defined by two anchors (segment endpoints, polyline vertices, wall ends...).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct LineRef {
    /// Start anchor.
    pub from: AnchorRef,
    /// End anchor.
    pub to: AnchorRef,
}

impl LineRef {
    /// `start → end` of a line-like entity.
    #[must_use]
    pub fn of(entity: EntityId) -> Self {
        Self { from: AnchorRef::new(entity, "start"), to: AnchorRef::new(entity, "end") }
    }
}

/// Comparison operator for linear and expression rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Cmp {
    /// `=`
    #[serde(rename = "=")]
    Eq,
    /// `<=`
    #[serde(rename = "<=")]
    Le,
    /// `>=`
    #[serde(rename = ">=")]
    Ge,
}

/// One linear term `coef · param`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Term {
    /// Coefficient.
    pub coef: f64,
    /// Parameter.
    pub param: ParamRef,
}

/// Constraint kinds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum RuleSpec {
    /// `param = value`.
    Fix {
        /// Parameter.
        param: ParamRef,
        /// Value (canonical units).
        value: f64,
    },
    /// `a = b`.
    Equal {
        /// First parameter.
        a: ParamRef,
        /// Second parameter.
        b: ParamRef,
    },
    /// All parameters equal.
    AllEqual {
        /// Parameters.
        params: Vec<ParamRef>,
    },
    /// `Σ coef·param (op) rhs` — sums, differences, minimums, maximums.
    Linear {
        /// Terms.
        terms: Vec<Term>,
        /// Operator.
        op: Cmp,
        /// Right-hand side.
        rhs: f64,
    },
    /// `a = k · b`.
    Ratio {
        /// Numerator parameter.
        a: ParamRef,
        /// Denominator parameter.
        b: ParamRef,
        /// Constant ratio.
        k: f64,
    },
    /// Consecutive differences equal (equally spaced values).
    EqualSpacing {
        /// Parameters in order.
        params: Vec<ParamRef>,
    },
    /// Coincident anchors.
    Coincident {
        /// First anchor.
        a: AnchorRef,
        /// Second anchor.
        b: AnchorRef,
    },
    /// Two anchors share Y.
    Horizontal {
        /// First anchor.
        a: AnchorRef,
        /// Second anchor.
        b: AnchorRef,
    },
    /// Two anchors share X.
    Vertical {
        /// First anchor.
        a: AnchorRef,
        /// Second anchor.
        b: AnchorRef,
    },
    /// Anchor fixed at a point.
    FixPoint {
        /// Anchor.
        a: AnchorRef,
        /// Location (world coordinates).
        at: Point,
    },
    /// Distance between anchors.
    Distance {
        /// First anchor.
        a: AnchorRef,
        /// Second anchor.
        b: AnchorRef,
        /// Distance (mm, ≥ 0).
        value: f64,
    },
    /// Signed distance from an anchor to a line (positive = left of the line).
    PointLineDistance {
        /// Point.
        point: AnchorRef,
        /// Line.
        line: LineRef,
        /// Signed distance (mm).
        value: f64,
    },
    /// Anchor on the infinite line.
    PointOnLine {
        /// Point.
        point: AnchorRef,
        /// Line.
        line: LineRef,
    },
    /// Anchor on a circle/arc entity.
    PointOnCircle {
        /// Point.
        point: AnchorRef,
        /// Circle or arc entity.
        circle: EntityId,
    },
    /// Line length.
    Length {
        /// Line.
        line: LineRef,
        /// Length (mm).
        value: f64,
    },
    /// Equal line lengths.
    EqualLength {
        /// First line.
        a: LineRef,
        /// Second line.
        b: LineRef,
    },
    /// Parallel lines.
    Parallel {
        /// First line.
        a: LineRef,
        /// Second line.
        b: LineRef,
    },
    /// Perpendicular lines.
    Perpendicular {
        /// First line.
        a: LineRef,
        /// Second line.
        b: LineRef,
    },
    /// Signed angle from `a` to `b`.
    Angle {
        /// First line.
        a: LineRef,
        /// Second line.
        b: LineRef,
        /// Angle (radians).
        value: f64,
    },
    /// Circles/arcs share their center.
    Concentric {
        /// First circle/arc.
        a: EntityId,
        /// Second circle/arc.
        b: EntityId,
    },
    /// Radius value.
    Radius {
        /// Circle/arc.
        circle: EntityId,
        /// Radius (mm).
        value: f64,
    },
    /// Equal radii.
    EqualRadius {
        /// First circle/arc.
        a: EntityId,
        /// Second circle/arc.
        b: EntityId,
    },
    /// Line tangent to circle; `side` (±1) is the side of the line the circle is on.
    TangentLineCircle {
        /// Line.
        line: LineRef,
        /// Circle/arc.
        circle: EntityId,
        /// +1 left of the line, −1 right.
        side: f64,
    },
    /// Circle–circle tangency.
    TangentCircles {
        /// First circle/arc.
        a: EntityId,
        /// Second circle/arc.
        b: EntityId,
        /// Internal tangency (one inside the other).
        #[serde(default)]
        internal: bool,
        /// For internal tangency: +1 when `a` is the outer circle.
        #[serde(default = "plus_one")]
        sign: f64,
    },
    /// Expression rule in the Dotloom expression language, evaluated in Rust:
    /// `lhs (op) rhs`, e.g. `"self.offset + self.width" <= "host.length"`.
    Expression {
        /// Entity whose namespace (`self`, references) the expressions use.
        entity: EntityId,
        /// Left expression.
        lhs: String,
        /// Operator.
        op: Cmp,
        /// Right expression.
        rhs: String,
    },
}

fn plus_one() -> f64 {
    1.0
}

impl RuleSpec {
    /// Entities referenced by the rule.
    #[must_use]
    pub fn entities(&self) -> Vec<EntityId> {
        let mut v = Vec::new();
        let line = |l: &LineRef, v: &mut Vec<EntityId>| {
            v.push(l.from.entity);
            v.push(l.to.entity);
        };
        match self {
            Self::Fix { param, .. } => v.push(param.entity),
            Self::Equal { a, b } | Self::Ratio { a, b, .. } => {
                v.push(a.entity);
                v.push(b.entity);
            }
            Self::AllEqual { params } | Self::EqualSpacing { params } => v.extend(params.iter().map(|p| p.entity)),
            Self::Linear { terms, .. } => v.extend(terms.iter().map(|t| t.param.entity)),
            Self::Coincident { a, b }
            | Self::Horizontal { a, b }
            | Self::Vertical { a, b }
            | Self::Distance { a, b, .. } => {
                v.push(a.entity);
                v.push(b.entity);
            }
            Self::FixPoint { a, .. } => v.push(a.entity),
            Self::PointLineDistance { point, line: l, .. } | Self::PointOnLine { point, line: l } => {
                v.push(point.entity);
                line(l, &mut v);
            }
            Self::PointOnCircle { point, circle } => {
                v.push(point.entity);
                v.push(*circle);
            }
            Self::Length { line: l, .. } => line(l, &mut v),
            Self::EqualLength { a, b }
            | Self::Parallel { a, b }
            | Self::Perpendicular { a, b }
            | Self::Angle { a, b, .. } => {
                line(a, &mut v);
                line(b, &mut v);
            }
            Self::Concentric { a, b } | Self::EqualRadius { a, b } | Self::TangentCircles { a, b, .. } => {
                v.push(*a);
                v.push(*b);
            }
            Self::Radius { circle, .. } => v.push(*circle),
            Self::TangentLineCircle { line: l, circle, .. } => {
                line(l, &mut v);
                v.push(*circle);
            }
            Self::Expression { entity, .. } => v.push(*entity),
        }
        v.sort_unstable();
        v.dedup();
        v
    }

    /// Anchor references used by the rule.
    #[must_use]
    pub fn anchors(&self) -> Vec<&AnchorRef> {
        match self {
            Self::Coincident { a, b }
            | Self::Horizontal { a, b }
            | Self::Vertical { a, b }
            | Self::Distance { a, b, .. } => {
                vec![a, b]
            }
            Self::FixPoint { a, .. } => vec![a],
            Self::PointLineDistance { point, line, .. } | Self::PointOnLine { point, line } => {
                vec![point, &line.from, &line.to]
            }
            Self::PointOnCircle { point, .. } => vec![point],
            Self::Length { line, .. } | Self::TangentLineCircle { line, .. } => vec![&line.from, &line.to],
            Self::EqualLength { a, b }
            | Self::Parallel { a, b }
            | Self::Perpendicular { a, b }
            | Self::Angle { a, b, .. } => {
                vec![&a.from, &a.to, &b.from, &b.to]
            }
            _ => Vec::new(),
        }
    }

    /// Parameter references used by the rule.
    #[must_use]
    pub fn params(&self) -> Vec<&ParamRef> {
        match self {
            Self::Fix { param, .. } => vec![param],
            Self::Equal { a, b } | Self::Ratio { a, b, .. } => vec![a, b],
            Self::AllEqual { params } | Self::EqualSpacing { params } => params.iter().collect(),
            Self::Linear { terms, .. } => terms.iter().map(|t| &t.param).collect(),
            _ => Vec::new(),
        }
    }

    /// Remap entity references (used by copy/paste). Returns `false` if any
    /// referenced entity has no mapping.
    pub fn remap(&mut self, map: &BTreeMap<EntityId, EntityId>) -> bool {
        let mut ok = true;
        let mut m = |e: &mut EntityId| match map.get(e) {
            Some(n) => *e = *n,
            None => ok = false,
        };
        match self {
            Self::Fix { param, .. } => m(&mut param.entity),
            Self::Equal { a, b } | Self::Ratio { a, b, .. } => {
                m(&mut a.entity);
                m(&mut b.entity);
            }
            Self::AllEqual { params } | Self::EqualSpacing { params } => {
                params.iter_mut().for_each(|p| m(&mut p.entity))
            }
            Self::Linear { terms, .. } => terms.iter_mut().for_each(|t| m(&mut t.param.entity)),
            Self::Coincident { a, b }
            | Self::Horizontal { a, b }
            | Self::Vertical { a, b }
            | Self::Distance { a, b, .. } => {
                m(&mut a.entity);
                m(&mut b.entity);
            }
            Self::FixPoint { a, .. } => m(&mut a.entity),
            Self::PointLineDistance { point, line, .. } | Self::PointOnLine { point, line } => {
                m(&mut point.entity);
                m(&mut line.from.entity);
                m(&mut line.to.entity);
            }
            Self::PointOnCircle { point, circle } => {
                m(&mut point.entity);
                m(circle);
            }
            Self::Length { line, .. } => {
                m(&mut line.from.entity);
                m(&mut line.to.entity);
            }
            Self::EqualLength { a, b }
            | Self::Parallel { a, b }
            | Self::Perpendicular { a, b }
            | Self::Angle { a, b, .. } => {
                m(&mut a.from.entity);
                m(&mut a.to.entity);
                m(&mut b.from.entity);
                m(&mut b.to.entity);
            }
            Self::Concentric { a, b } | Self::EqualRadius { a, b } | Self::TangentCircles { a, b, .. } => {
                m(a);
                m(b);
            }
            Self::Radius { circle, .. } => m(circle),
            Self::TangentLineCircle { line, circle, .. } => {
                m(&mut line.from.entity);
                m(&mut line.to.entity);
                m(circle);
            }
            Self::Expression { entity, .. } => m(entity),
        }
        ok
    }

    /// Stable kind name.
    #[must_use]
    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::Fix { .. } => "fix",
            Self::Equal { .. } => "equal",
            Self::AllEqual { .. } => "allEqual",
            Self::Linear { .. } => "linear",
            Self::Ratio { .. } => "ratio",
            Self::EqualSpacing { .. } => "equalSpacing",
            Self::Coincident { .. } => "coincident",
            Self::Horizontal { .. } => "horizontal",
            Self::Vertical { .. } => "vertical",
            Self::FixPoint { .. } => "fixPoint",
            Self::Distance { .. } => "distance",
            Self::PointLineDistance { .. } => "pointLineDistance",
            Self::PointOnLine { .. } => "pointOnLine",
            Self::PointOnCircle { .. } => "pointOnCircle",
            Self::Length { .. } => "length",
            Self::EqualLength { .. } => "equalLength",
            Self::Parallel { .. } => "parallel",
            Self::Perpendicular { .. } => "perpendicular",
            Self::Angle { .. } => "angle",
            Self::Concentric { .. } => "concentric",
            Self::Radius { .. } => "radius",
            Self::EqualRadius { .. } => "equalRadius",
            Self::TangentLineCircle { .. } => "tangentLineCircle",
            Self::TangentCircles { .. } => "tangentCircles",
            Self::Expression { .. } => "expression",
        }
    }

    /// Numeric fields are finite and within their domain.
    #[must_use]
    pub fn values_valid(&self) -> bool {
        match self {
            Self::Fix { value, .. } | Self::PointLineDistance { value, .. } | Self::Angle { value, .. } => {
                value.is_finite()
            }
            Self::Distance { value, .. } | Self::Length { value, .. } | Self::Radius { value, .. } => {
                value.is_finite() && *value >= 0.0
            }
            Self::Linear { terms, rhs, .. } => rhs.is_finite() && terms.iter().all(|t| t.coef.is_finite()),
            Self::Ratio { k, .. } => k.is_finite(),
            Self::FixPoint { at, .. } => at.is_finite(),
            Self::TangentLineCircle { side, .. } => *side == 1.0 || *side == -1.0,
            Self::TangentCircles { sign, .. } => *sign == 1.0 || *sign == -1.0,
            Self::Expression { lhs, rhs, .. } => lhs.len() <= 4096 && rhs.len() <= 4096,
            _ => true,
        }
    }
}

/// Strength as stored in documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StrengthSpec {
    /// Weak preference.
    Weak,
    /// Medium preference.
    Medium,
    /// Strong preference.
    Strong,
    /// Hard rule (default).
    #[default]
    Required,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_required(s: &StrengthSpec) -> bool {
    *s == StrengthSpec::Required
}

fn yes() -> bool {
    true
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_true(v: &bool) -> bool {
    *v
}

/// A stored constraint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Constraint {
    /// Identifier.
    pub id: ConstraintId,
    /// What is constrained.
    pub rule: RuleSpec,
    /// Strength (hard by default).
    #[serde(default, skip_serializing_if = "is_required")]
    pub strength: StrengthSpec,
    /// Disabled constraints are kept but ignored by the solver.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    /// User-visible label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Source (`user`, `plugin:acme.wall`, `template:...`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Owning entity for template-generated constraints (deleted with it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<EntityId>,
    /// Unknown fields preserved.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Constraint {
    /// Hard, enabled user constraint.
    #[must_use]
    pub fn new(id: ConstraintId, rule: RuleSpec) -> Self {
        Self {
            id,
            rule,
            strength: StrengthSpec::Required,
            enabled: true,
            label: None,
            source: None,
            owner: None,
            extra: BTreeMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_json_shape() {
        let c = Constraint::new(
            ConstraintId(9),
            RuleSpec::Distance {
                a: AnchorRef::new(EntityId(1), "start"),
                b: AnchorRef::new(EntityId(2), "end"),
                value: 50.0,
            },
        );
        let j = serde_json::to_string(&c).unwrap();
        assert_eq!(
            j,
            r#"{"id":9,"rule":{"kind":"distance","a":{"entity":1,"anchor":"start"},"b":{"entity":2,"anchor":"end"},"value":50.0}}"#
        );
        let back: Constraint = serde_json::from_str(&j).unwrap();
        assert_eq!(back, c);
        let p = ParamRef::prop(EntityId(3), "width");
        assert_eq!(serde_json::to_string(&p).unwrap(), r#"{"entity":3,"prop":"width"}"#);
        let lin = RuleSpec::Linear { terms: vec![Term { coef: 1.0, param: p }], op: Cmp::Ge, rhs: 400.0 };
        assert!(serde_json::to_string(&lin).unwrap().contains(r#""op":">=""#));
    }

    #[test]
    fn unknown_fields_survive() {
        let j = r#"{"id":1,"rule":{"kind":"radius","circle":4,"value":5.0},"futureFlag":{"x":1}}"#;
        let c: Constraint = serde_json::from_str(j).unwrap();
        assert!(c.extra.contains_key("futureFlag"));
        let back = serde_json::to_string(&c).unwrap();
        assert!(back.contains("futureFlag"));
    }

    #[test]
    fn remap_reports_missing() {
        let mut r = RuleSpec::Parallel { a: LineRef::of(EntityId(1)), b: LineRef::of(EntityId(2)) };
        let map: BTreeMap<_, _> = [(EntityId(1), EntityId(11))].into_iter().collect();
        assert!(!r.remap(&map));
        let full: BTreeMap<_, _> = [(EntityId(1), EntityId(11)), (EntityId(2), EntityId(12))].into_iter().collect();
        let mut r2 = RuleSpec::Parallel { a: LineRef::of(EntityId(1)), b: LineRef::of(EntityId(2)) };
        assert!(r2.remap(&full));
        assert_eq!(r2.entities(), vec![EntityId(11), EntityId(12)]);
    }
}

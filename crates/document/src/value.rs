//! Typed property values, colors and styles.

use core::fmt;

use dotloom_geometry::Point;
use serde::{Deserialize, Serialize};

use crate::{DocError, EntityId};

/// Reference to another entity: `{"ref": 12}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RefValue {
    /// Target entity.
    #[serde(rename = "ref")]
    pub entity: EntityId,
}

/// A named anchor of an entity: `{"entity": 12, "anchor": "start"}`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AnchorRef {
    /// Entity.
    pub entity: EntityId,
    /// Anchor name (`start`, `end`, `mid`, `center`, `v3`, plugin-defined names).
    pub anchor: String,
}

impl AnchorRef {
    /// Create an anchor reference.
    #[must_use]
    pub fn new(entity: EntityId, anchor: impl Into<String>) -> Self {
        Self { entity, anchor: anchor.into() }
    }
}

/// A typed property value. Numbers are stored in canonical units (mm, rad, s); the
/// dimension of each property comes from its type definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PropValue {
    /// Boolean.
    Bool(bool),
    /// Number in canonical units.
    Number(f64),
    /// String or enum value.
    Text(String),
    /// Point `[x, y]`.
    Point(Point),
    /// Entity reference.
    Ref(RefValue),
    /// Anchor reference.
    Anchor(AnchorRef),
}

impl PropValue {
    /// Number value.
    #[must_use]
    pub fn as_number(&self) -> Option<f64> {
        if let Self::Number(v) = self { Some(*v) } else { None }
    }

    /// Referenced entity, if this is a reference.
    #[must_use]
    pub fn referenced_entity(&self) -> Option<EntityId> {
        match self {
            Self::Ref(r) => Some(r.entity),
            Self::Anchor(a) => Some(a.entity),
            _ => None,
        }
    }

    /// Whether all numbers are finite.
    #[must_use]
    pub fn is_finite(&self) -> bool {
        match self {
            Self::Number(v) => v.is_finite(),
            Self::Point(p) => p.is_finite(),
            _ => true,
        }
    }

    /// Kind name for diagnostics.
    #[must_use]
    pub const fn kind_name(&self) -> &'static str {
        match self {
            Self::Bool(_) => "bool",
            Self::Number(_) => "number",
            Self::Text(_) => "text",
            Self::Point(_) => "point",
            Self::Ref(_) => "ref",
            Self::Anchor(_) => "anchor",
        }
    }
}

/// RGBA color, serialized as `#rrggbb` or `#rrggbbaa`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Color(pub u32);

impl Color {
    /// Opaque black.
    pub const BLACK: Self = Self(0x0000_00ff);

    /// From components.
    #[must_use]
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self(((r as u32) << 24) | ((g as u32) << 16) | ((b as u32) << 8) | a as u32)
    }

    /// Components `[r, g, b, a]`.
    #[must_use]
    pub const fn components(self) -> [u8; 4] {
        self.0.to_be_bytes()
    }

    /// Parse `#rgb`, `#rrggbb` or `#rrggbbaa`.
    pub fn parse(s: &str) -> Result<Self, DocError> {
        let hex = s.strip_prefix('#').ok_or_else(|| DocError::InvalidValue(format!("color `{s}`")))?;
        let bad = || DocError::InvalidValue(format!("color `{s}`"));
        if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(bad());
        }
        let v = u32::from_str_radix(hex, 16).map_err(|_| bad())?;
        match hex.len() {
            3 => {
                let r = (v >> 8) & 0xf;
                let g = (v >> 4) & 0xf;
                let b = v & 0xf;
                Ok(Self((r * 17) << 24 | (g * 17) << 16 | (b * 17) << 8 | 0xff))
            }
            6 => Ok(Self(v << 8 | 0xff)),
            8 => Ok(Self(v)),
            _ => Err(bad()),
        }
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [r, g, b, a] = self.components();
        if a == 0xff { write!(f, "#{r:02x}{g:02x}{b:02x}") } else { write!(f, "#{r:02x}{g:02x}{b:02x}{a:02x}") }
    }
}

impl TryFrom<String> for Color {
    type Error = DocError;
    fn try_from(s: String) -> Result<Self, DocError> {
        Self::parse(&s)
    }
}

impl From<Color> for String {
    fn from(c: Color) -> Self {
        c.to_string()
    }
}

/// Visual style of an entity. Unset fields inherit from the layer, then the theme.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Style {
    /// Stroke color.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<Color>,
    /// Fill color (regions only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Color>,
    /// Stroke width in CSS pixels (screen-constant).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_width: Option<f64>,
    /// Dash pattern in CSS pixels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dash: Option<Vec<f64>>,
}

impl Style {
    /// Whether every field is unset.
    #[must_use]
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Validate numeric fields.
    pub fn validate(&self) -> Result<(), DocError> {
        if let Some(w) = self.stroke_width
            && !(w.is_finite() && (0.0..=1000.0).contains(&w))
        {
            return Err(DocError::InvalidValue("stroke width must be within 0..=1000 px".into()));
        }
        if let Some(d) = &self.dash
            && (d.len() > 16 || d.iter().any(|v| !(v.is_finite() && *v >= 0.0)))
        {
            return Err(DocError::InvalidValue("dash pattern".into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prop_values_roundtrip_untagged() {
        let vals = vec![
            PropValue::Bool(true),
            PropValue::Number(600.0),
            PropValue::Text("oak".into()),
            PropValue::Point(Point::new(1.0, 2.0)),
            PropValue::Ref(RefValue { entity: EntityId(4) }),
            PropValue::Anchor(AnchorRef::new(EntityId(4), "start")),
        ];
        let j = serde_json::to_string(&vals).unwrap();
        assert_eq!(j, r#"[true,600.0,"oak",[1.0,2.0],{"ref":4},{"entity":4,"anchor":"start"}]"#);
        let back: Vec<PropValue> = serde_json::from_str(&j).unwrap();
        assert_eq!(back, vals);
    }

    #[test]
    fn colors() {
        assert_eq!(Color::parse("#fff").unwrap(), Color(0xffff_ffff));
        assert_eq!(Color::parse("#1a2b3c").unwrap().to_string(), "#1a2b3c");
        assert_eq!(Color::parse("#1a2b3c80").unwrap().to_string(), "#1a2b3c80");
        assert!(Color::parse("1a2b3c").is_err());
        assert!(Color::parse("#12345").is_err());
        assert!(Color::parse("#+12345").is_err());
    }
}

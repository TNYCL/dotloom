//! Colors and themes.

use serde::{Deserialize, Serialize};

/// Packed `0xRRGGBBAA` color (the scene contract's `Rgba`).
pub type Rgba = u32;

/// Unpack to straight-alpha components in `0..=1`.
#[must_use]
pub fn unpack(c: Rgba) -> [f32; 4] {
    [
        ((c >> 24) & 0xff) as f32 / 255.0,
        ((c >> 16) & 0xff) as f32 / 255.0,
        ((c >> 8) & 0xff) as f32 / 255.0,
        (c & 0xff) as f32 / 255.0,
    ]
}

/// Pack straight-alpha components.
#[must_use]
pub fn pack(c: [f32; 4]) -> Rgba {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    (q(c[0]) << 24) | (q(c[1]) << 16) | (q(c[2]) << 8) | q(c[3])
}

/// Multiply the alpha channel.
#[must_use]
pub fn with_alpha(c: Rgba, factor: f32) -> Rgba {
    let mut v = unpack(c);
    v[3] *= factor;
    pack(v)
}

/// Convert to the vertex format: premultiplied RGBA8, little-endian `[r, g, b, a]`.
#[must_use]
pub fn premul_bytes(c: Rgba) -> u32 {
    let [r, g, b, a] = unpack(c);
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    q(r * a) | (q(g * a) << 8) | (q(b * a) << 16) | (q(a) << 24)
}

/// Parse `#rgb`, `#rrggbb` or `#rrggbbaa`.
#[must_use]
pub fn parse_hex(s: &str) -> Option<Rgba> {
    let h = s.strip_prefix('#')?;
    let v = u32::from_str_radix(h, 16).ok()?;
    match h.len() {
        3 => {
            let (r, g, b) = ((v >> 8) & 0xf, (v >> 4) & 0xf, v & 0xf);
            Some(((r * 17) << 24) | ((g * 17) << 16) | ((b * 17) << 8) | 0xff)
        }
        6 => Some((v << 8) | 0xff),
        8 => Some(v),
        _ => None,
    }
}

/// Renderer colors. Scene color `0` means [`Theme::foreground`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Theme {
    /// Canvas background.
    pub background: Rgba,
    /// Default stroke/text color (scene color `0`).
    pub foreground: Rgba,
    /// Selected items.
    pub selection: Rgba,
    /// Hovered item.
    pub hover: Rgba,
    /// Items with constraint problems.
    pub problem: Rgba,
    /// Minor grid lines.
    pub grid_minor: Rgba,
    /// Major grid lines.
    pub grid_major: Rgba,
    /// Grid axes through the origin.
    pub grid_axis: Rgba,
    /// Snap markers and selection grips.
    pub marker: Rgba,
    /// Window selection rectangle.
    pub marquee_window: Rgba,
    /// Crossing selection rectangle.
    pub marquee_crossing: Rgba,
    /// Construction guides.
    pub guide: Rgba,
    /// In-progress drawing preview.
    pub sketch: Rgba,
}

impl Default for Theme {
    fn default() -> Self {
        Self::light()
    }
}

impl Theme {
    /// Light theme (dark ink on white).
    #[must_use]
    pub const fn light() -> Self {
        Self {
            background: 0xffff_ffff,
            foreground: 0x1f23_28ff,
            selection: 0x0a66_d9ff,
            hover: 0x4d94_ffff,
            problem: 0xd12d_2dff,
            grid_minor: 0xe9ec_f0ff,
            grid_major: 0xd3d8_deff,
            grid_axis: 0xb0b8_c2ff,
            marker: 0xe0_6c00ff,
            marquee_window: 0x0a66_d9ff,
            marquee_crossing: 0x1e9e_4aff,
            guide: 0x8a63_d2ff,
            sketch: 0x0a66_d9ff,
        }
    }

    /// Dark theme (light ink on near-black).
    #[must_use]
    pub const fn dark() -> Self {
        Self {
            background: 0x1518_1cff,
            foreground: 0xe6ea_eeff,
            selection: 0x5aa2_ffff,
            hover: 0x8bbf_ffff,
            problem: 0xff6b_6bff,
            grid_minor: 0x2025_2bff,
            grid_major: 0x2c33_3bff,
            grid_axis: 0x4a54_60ff,
            marker: 0xffa6_33ff,
            marquee_window: 0x5aa2_ffff,
            marquee_crossing: 0x4cd3_7aff,
            guide: 0xb59c_ffff,
            sketch: 0x5aa2_ffff,
        }
    }

    /// Resolve a scene color (`0` = foreground).
    #[must_use]
    pub fn resolve(&self, c: Rgba) -> Rgba {
        if c == 0 { self.foreground } else { c }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_roundtrip_and_hex() {
        assert_eq!(pack(unpack(0x1234_56ff)), 0x1234_56ff);
        assert_eq!(parse_hex("#fff"), Some(0xffff_ffff));
        assert_eq!(parse_hex("#102030"), Some(0x1020_30ff));
        assert_eq!(parse_hex("#10203040"), Some(0x1020_3040));
        assert_eq!(parse_hex("102030"), None);
    }

    #[test]
    fn premultiplied_layout() {
        // 50% red: r = 0.5 after premultiplication, stored first.
        let v = premul_bytes(0xff00_0080);
        assert_eq!(v & 0xff, 128);
        assert_eq!(v >> 24, 128);
        assert_eq!((v >> 8) & 0xff, 0);
    }
}

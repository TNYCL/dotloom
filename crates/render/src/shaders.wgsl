// Dotloom renderer shaders.
//
// Only features available on WebGL2 (GLSL ES 3.0) are used: uniform buffers,
// instancing, vertex_index, one filterable R8 texture. No storage buffers or
// compute, so the WebGPU and WebGL2 backends run the same code.

// Per-batch transform: local units -> device pixels (y down).
struct Xf {
    // 2x2 linear part (a, b, c, d): px = (a*x + c*y + e, b*x + d*y + f).
    m: vec4<f32>,
    // (e, f, viewport width px, viewport height px)
    tv: vec4<f32>,
    // (dpr, device px per local unit, alpha, unused)
    p: vec4<f32>,
};

@group(0) @binding(0) var<uniform> xf: Xf;

fn to_px(p: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(xf.m.x * p.x + xf.m.z * p.y + xf.tv.x, xf.m.y * p.x + xf.m.w * p.y + xf.tv.y);
}

fn to_clip(px: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(px.x / xf.tv.z * 2.0 - 1.0, 1.0 - px.y / xf.tv.w * 2.0, 0.0, 1.0);
}

// ---------------------------------------------------------------------------
// Lines: screen-constant width capsules with analytic anti-aliasing and
// screen-space dashes.

struct LineIn {
    @location(0) p0: vec2<f32>,
    @location(1) p1: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) width: f32,
    @location(4) dist0: f32,
    @location(5) dash: vec4<f32>,
};

struct LineOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
    // (along, across) relative to p0, device px
    @location(1) local: vec2<f32>,
    // (length px, half width px, coverage factor)
    @location(2) geom: vec3<f32>,
    @location(3) dash: vec4<f32>,
    @location(4) dist0: f32,
};

@vertex
fn vs_line(@builtin(vertex_index) vi: u32, l: LineIn) -> LineOut {
    let a = to_px(l.p0);
    let b = to_px(l.p1);
    let w = l.width * xf.p.x;
    let hw = max(w, 1.0) * 0.5;
    let ext = hw + 1.0;
    let d = b - a;
    let len = length(d);
    var u = vec2<f32>(1.0, 0.0);
    if (len > 1e-4) {
        u = d / len;
    }
    let n = vec2<f32>(-u.y, u.x);
    var s = 0.0;
    var side = -1.0;
    switch vi {
        case 1u: { s = 1.0; }
        case 2u, 4u: { s = 1.0; side = 1.0; }
        case 5u: { side = 1.0; }
        default: {}
    }
    let px = mix(a - u * ext, b + u * ext, s) + n * side * ext;
    var out: LineOut;
    out.pos = to_clip(px);
    out.color = l.color * xf.p.z;
    out.local = vec2<f32>(dot(px - a, u), dot(px - a, n));
    // Lines thinner than one device pixel fade instead of disappearing.
    out.geom = vec3<f32>(len, hw, min(w, 1.0));
    out.dash = l.dash * xf.p.x;
    out.dist0 = l.dist0 * xf.p.y;
    return out;
}

@fragment
fn fs_line(i: LineOut) -> @location(0) vec4<f32> {
    let len = i.geom.x;
    let hw = i.geom.y;
    let t = clamp(i.local.x, 0.0, len);
    let dist = length(vec2<f32>(i.local.x - t, i.local.y));
    var a = clamp(hw + 0.5 - dist, 0.0, 1.0) * i.geom.z;
    if (i.dash.x > 0.0) {
        let period = i.dash.x + i.dash.y + i.dash.z + i.dash.w;
        if (period > 0.0) {
            let m = (i.dist0 + t) % period;
            let on1 = m < i.dash.x;
            let on2 = m >= i.dash.x + i.dash.y && m < i.dash.x + i.dash.y + i.dash.z;
            if (!(on1 || on2)) {
                a = 0.0;
            }
        }
    }
    if (a <= 0.0) {
        discard;
    }
    return i.color * a;
}

// ---------------------------------------------------------------------------
// Fills: plain triangles (anti-aliased by MSAA when available).

struct FillOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_fill(@location(0) p: vec2<f32>, @location(1) color: vec4<f32>) -> FillOut {
    var out: FillOut;
    out.pos = to_clip(to_px(p));
    out.color = color * xf.p.z;
    return out;
}

@fragment
fn fs_fill(i: FillOut) -> @location(0) vec4<f32> {
    return i.color;
}

// ---------------------------------------------------------------------------
// Glyphs: signed distance field quads.

@group(1) @binding(0) var atlas: texture_2d<f32>;
@group(1) @binding(1) var atlas_sampler: sampler;

struct GlyphIn {
    @location(0) origin: vec2<f32>,
    @location(1) axis_x: vec2<f32>,
    @location(2) axis_y: vec2<f32>,
    @location(3) uv: vec4<f32>,
    @location(4) color: vec4<f32>,
    @location(5) height: f32,
};

struct GlyphOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
};

@vertex
fn vs_glyph(@builtin(vertex_index) vi: u32, g: GlyphIn) -> GlyphOut {
    var cx = 0.0;
    var cy = 0.0;
    switch vi {
        case 1u: { cx = 1.0; }
        case 2u, 4u: { cx = 1.0; cy = 1.0; }
        case 5u: { cy = 1.0; }
        default: {}
    }
    var out: GlyphOut;
    let p = g.origin + g.axis_x * cx + g.axis_y * cy;
    out.pos = to_clip(to_px(p));
    // Text below one device pixel is not legible: collapse the quad.
    if (g.height * xf.p.y < 1.0) {
        out.pos = vec4<f32>(-2.0, -2.0, 0.0, 1.0);
    }
    out.color = g.color * xf.p.z;
    out.uv = vec2<f32>(mix(g.uv.x, g.uv.z, cx), mix(g.uv.w, g.uv.y, cy));
    return out;
}

@fragment
fn fs_glyph(i: GlyphOut) -> @location(0) vec4<f32> {
    let d = textureSample(atlas, atlas_sampler, i.uv).r;
    let w = max(fwidth(d) * 0.7, 1e-4);
    let a = smoothstep(0.5 - w, 0.5 + w, d);
    if (a <= 0.0) {
        discard;
    }
    return i.color * a;
}

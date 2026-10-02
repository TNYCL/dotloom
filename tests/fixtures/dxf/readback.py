"""Read Dotloom's DXF exports back with ezdxf, an independent implementation (CI).

Usage:  python tests/fixtures/dxf/readback.py <path to the dotloom CLI> <work dir>

For every ezdxf-written fixture next to this script: import it with Dotloom
(.dxf -> .dotl), export it again (.dotl -> .dxf) and read the export with ezdxf.

Checks per fixture:
- the export loads and `audit()` reports no errors;
- every supported source entity (LINE, CIRCLE, ARC, LWPOLYLINE, POLYLINE, POINT)
  has an exported counterpart whose geometry, sampled in world coordinates and
  converted to millimetres, lies within 0.006 mm (symmetric Hausdorff distance of
  polylines sampled with a 0.002 mm chord error);
- every TEXT/MTEXT string survives (Turkish letters included; line breaks become
  spaces, which the exporter reports) at the same world position;
- the export declares millimetres ($INSUNITS = 4).
"""

import os
import re
import subprocess
import sys

import ezdxf
from ezdxf import path as dxfpath

HERE = os.path.dirname(os.path.abspath(__file__))
# Millimetres per drawing unit for $INSUNITS values used by the fixtures.
MM = {0: 1.0, 1: 25.4, 2: 304.8, 4: 1.0, 5: 10.0, 6: 1000.0}
CURVES = {"LINE", "CIRCLE", "ARC", "LWPOLYLINE", "POLYLINE", "POINT"}
# Curves are compared as polylines sampled with a chord error of FLATTEN mm on both
# sides; the comparison tolerance allows for the two sampling errors.
FLATTEN = 0.002
TOL = 3 * FLATTEN


def samples(e, scale):
    if e.dxftype() == "POINT":
        p = e.ocs().to_wcs(e.dxf.location)
        return [(p.x * scale, p.y * scale)]
    pts = dxfpath.make_path(e).flattening(distance=FLATTEN / scale)
    return [(p.x * scale, p.y * scale) for p in pts]


def bbox(pts):
    xs = [p[0] for p in pts]
    ys = [p[1] for p in pts]
    return (min(xs), min(ys), max(xs), max(ys))


def seg_dist(p, a, b):
    ax, ay = a
    bx, by = b
    dx, dy = bx - ax, by - ay
    L = dx * dx + dy * dy
    t = 0.0 if L == 0 else max(0.0, min(1.0, ((p[0] - ax) * dx + (p[1] - ay) * dy) / L))
    return ((p[0] - ax - t * dx) ** 2 + (p[1] - ay - t * dy) ** 2) ** 0.5


def to_curve(xs, ys):
    """Largest distance from the points `xs` to the polyline through `ys`."""
    if len(ys) == 1:
        return max(((x[0] - ys[0][0]) ** 2 + (x[1] - ys[0][1]) ** 2) ** 0.5 for x in xs)
    return max(min(seg_dist(x, ys[i], ys[i + 1]) for i in range(len(ys) - 1)) for x in xs)


def hausdorff(a, b):
    return max(to_curve(a, b), to_curve(b, a))


ESCAPE = re.compile(r"\\U\+([0-9A-Fa-f]{4})")


def normalize(text):
    r"""Decode \U+XXXX escapes; collapse whitespace (R12 TEXT is one line: Dotloom
    joins lines with spaces and reports it)."""
    return " ".join(ESCAPE.sub(lambda m: chr(int(m.group(1), 16)), text).split())


def texts(doc, scale):
    out = []
    for e in doc.modelspace():
        if e.dxftype() == "TEXT":
            p = e.ocs().to_wcs(e.dxf.insert)
            out.append((normalize(e.dxf.text), p.x * scale, p.y * scale))
        elif e.dxftype() == "MTEXT":
            p = e.dxf.insert
            out.append((normalize(e.plain_text()), p.x * scale, p.y * scale))
    return out


def check(cli, src, work):
    name = os.path.splitext(os.path.basename(src))[0]
    dotl = os.path.join(work, f"{name}.dotl")
    out = os.path.join(work, f"{name}.export.dxf")
    for a, b in ((src, dotl), (dotl, out)):
        subprocess.run([cli, "convert", a, b], check=True, capture_output=True)
    original = ezdxf.readfile(src)
    exported = ezdxf.readfile(out)
    problems = []
    auditor = exported.audit()
    if auditor.has_errors:
        problems.append(f"audit: {[str(e.message) for e in auditor.errors]}")
    if exported.header.get("$INSUNITS") != 4:
        problems.append(f"$INSUNITS {exported.header.get('$INSUNITS')} != 4")
    scale = MM.get(original.header.get("$INSUNITS", 0), 1.0)
    exported_curves = [samples(e, 1.0) for e in exported.modelspace() if e.dxftype() in CURVES]
    boxes = [bbox(c) for c in exported_curves]
    used = set()
    count = 0
    for e in original.modelspace():
        if e.dxftype() not in CURVES:
            continue
        count += 1
        want = samples(e, scale)
        wb = bbox(want)
        best = None
        for i, got in enumerate(exported_curves):
            if i in used or max(abs(a - b) for a, b in zip(wb, boxes[i])) > 1.0:
                continue
            d = hausdorff(want, got)
            if best is None or d < best[0]:
                best = (d, i)
        if best is None or best[0] > TOL:
            problems.append(f"{e.dxftype()} {e.dxf.handle}: nearest export {best[0] if best else 'none'} mm away")
        else:
            used.add(best[1])
    for text, x, y in texts(original, scale):
        found = [t for t in texts(exported, 1.0) if t[0] == text and abs(t[1] - x) <= TOL and abs(t[2] - y) <= TOL]
        if not found:
            problems.append(f"text {text!r} at ({x:.3f}, {y:.3f}) not found")
    status = "ok" if not problems else "FAILED"
    print(f"{status:6} {name}: {count} curves, {len(texts(original, scale))} texts")
    for p in problems:
        print(f"         {p}")
    return not problems


def main():
    cli, work = sys.argv[1], sys.argv[2]
    os.makedirs(work, exist_ok=True)
    fixtures = sorted(f for f in os.listdir(HERE) if f.endswith(".dxf"))
    results = [check(cli, os.path.join(HERE, f), work) for f in fixtures]
    print(f"ezdxf {ezdxf.__version__}: {sum(results)}/{len(results)} fixtures read back")
    sys.exit(0 if all(results) and results else 1)


if __name__ == "__main__":
    main()

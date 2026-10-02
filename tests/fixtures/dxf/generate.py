"""Generate DXF fixtures with ezdxf (MIT), an implementation independent of Dotloom.

Usage:  python -m pip install ezdxf==1.4.2 && python tests/fixtures/dxf/generate.py

The generated files are committed; re-running must produce equivalent geometry.
"""

import math
import os

import ezdxf
from ezdxf import units

HERE = os.path.dirname(os.path.abspath(__file__))


def basic(version: str, name: str, insunits: int) -> None:
    doc = ezdxf.new(version)
    doc.units = insunits
    doc.layers.add("WALLS", color=1)
    doc.layers.add("TEXT", color=5)
    msp = doc.modelspace()
    msp.add_line((0, 0), (100, 0), dxfattribs={"layer": "WALLS"})
    msp.add_line((100, 0), (100, 50), dxfattribs={"layer": "WALLS"})
    msp.add_circle((50, 25), 10)
    msp.add_arc((0, 0), 30, 0, 90)
    msp.add_text("Ölçü ğüşıİç", dxfattribs={"height": 5, "layer": "TEXT", "rotation": 30}).set_placement((10, 60))
    if version != "R12":
        # Bulge 1 = semicircle on the second segment.
        msp.add_lwpolyline([(0, 100, 0, 0, 0), (50, 100, 0, 0, 1), (100, 100, 0, 0, 0)], format="xyseb", close=False)
        msp.add_lwpolyline([(0, 150), (40, 150), (40, 190), (0, 190)], close=True)
        msp.add_mtext("multi\\Pline", dxfattribs={"char_height": 4, "insert": (0, 210)})
        msp.add_ellipse((200, 0), major_axis=(20, 0), ratio=0.5)
        msp.add_spline([(0, 0), (10, 10), (20, 0), (30, 10)])
        hatch = msp.add_hatch(color=2)
        hatch.paths.add_polyline_path([(300, 0), (320, 0), (320, 20)], is_closed=True)
    else:
        pl = msp.add_polyline2d([(0, 100), (50, 100), (100, 100)])
        pl.vertices[1].dxf.bulge = 1.0
    block = doc.blocks.new(name="DOOR")
    block.add_line((0, 0), (0, 10))
    msp.add_blockref("DOOR", (500, 0))
    msp.add_point((7, 7))
    doc.saveas(os.path.join(HERE, name))


def mirrored(name: str) -> None:
    """Circle and arc with extrusion (0, 0, -1): OCS mirrored in X."""
    doc = ezdxf.new("R2000")
    doc.units = units.MM
    msp = doc.modelspace()
    msp.add_circle((10, 5), 2, dxfattribs={"extrusion": (0, 0, -1)})
    msp.add_arc((10, 5), 4, 0, 90, dxfattribs={"extrusion": (0, 0, -1)})
    doc.saveas(os.path.join(HERE, name))


if __name__ == "__main__":
    basic("R12", "r12_basic.dxf", units.MM)
    basic("R2000", "r2000_cm.dxf", units.CM)
    basic("R2018", "r2018_mm.dxf", units.MM)
    mirrored("r2000_mirrored.dxf")
    print("ok")

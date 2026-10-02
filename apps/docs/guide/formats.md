# SVG, DXF and PNG

`.dotl` is the only format that keeps everything (rules, plugin objects, layers,
view state). The other formats are for exchange; every import and export returns a
**report** listing what was approximated or could not be carried.

## Support matrix

| | Import | Export |
|---|---|---|
| **SVG** | `svg`, `g`, `line`, `rect`, `circle`, `ellipse`, `polyline`, `polygon`, `path` (all commands; elliptical arcs become Béziers), `text`; nested transforms, `viewBox`, absolute units, `stroke`/`fill`/`stroke-width` attributes and inline `style` | all geometry in millimetres, one `<g>` per layer, styles, text |
| **DXF** (ASCII) | R12 (AC1009) … 2018 (AC1032): `LINE`, `LWPOLYLINE` (bulges), 2D `POLYLINE`/`VERTEX`, `CIRCLE`, `ARC`, `TEXT`, `MTEXT` (formatting stripped), `POINT`, layers; units from `$INSUNITS` | R12 (AC1009): `LINE`, `POLYLINE`/`VERTEX` (bulges), `CIRCLE`, `ARC`, `TEXT`, `POINT`, layers, `$INSUNITS = 4` (mm) |
| **PNG** | — | the renderer's output at a chosen width and background |

## Not carried (reported)

| Format | Reported losses |
|---|---|
| SVG export | rules, plugin parameters (drawn as plain geometry), associative dimensions |
| SVG import | `script`, `style` elements, `use`, `image`, `foreignObject`, gradients, filters, masks, clip paths (skipped); external references are never followed and nothing is inserted into the page DOM |
| DXF export | rules, plugin parameters, Béziers (flattened), fill colours |
| DXF import | `INSERT`, `HATCH`, `SPLINE`, `ELLIPSE`, `DIMENSION` and other entities are counted; unusual OCS orientations; unitless files are read as millimetres and reported |
| PNG export | everything editable |

Binary DXF and SVG DTDs are rejected. DXF text uses UTF-8 (AC1021+) or the
`$DWGCODEPAGE` ANSI_1252/ANSI_1254 code pages plus `\U+XXXX` escapes, so Turkish
text survives both directions.

The DXF fixtures in the repository are written by an independent library (ezdxf)
in several versions, and the exporter's output is read back by it in CI.

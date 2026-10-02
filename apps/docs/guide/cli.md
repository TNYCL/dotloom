# Command-line tool

`dotloom` inspects, validates and converts files without a browser or GPU.

```sh
cargo install dotloom-cli                     # or build from the repository
cargo install dotloom-cli --features png      # adds PNG export through the GPU renderer
```

```text
dotloom inspect  <file>                     summarize a .dotl, SVG or DXF file
dotloom validate <file.dotl>                container, schema, references and every hard rule
dotloom convert  <input> <output>           .dotl ⇄ .svg / .dxf, and → .png (feature png)
         [--width <px>] [--background <#rrggbb>]   PNG options
global:  --json                             machine-readable output
         --plugins <types.json>             plugin type definitions (repeatable)
```

## Exit codes

| Code | Meaning |
|---|---|
| 0 | success |
| 1 | invalid input, failed validation (violated rules are listed) or conversion |
| 2 | usage error |
| 3 | I/O error |
| 4 | capability not available in this build or on this machine (for example PNG without the `png` feature, or no GPU/software adapter) |

## Examples

```sh
dotloom --json validate ev-plani.dotl --plugins floorplan.json
dotloom convert plan.dxf plan.dotl
dotloom convert ev-plani.dotl ev-plani.svg
dotloom convert ev-plani.dotl ev-plani.png --width 1600 --background "#ffffff" --plugins floorplan.json
```

Every conversion prints (or returns as JSON) a report of what the target format
cannot carry — rules, plugin parameters, associative dimensions, unsupported
entities — so nothing is lost silently.

## PNG

PNG export uses the same wgpu renderer as the browser, offscreen. It needs a GPU or
a software adapter (Mesa lavapipe/llvmpipe on Linux, WARP on Windows). Only the PNG
command needs it; every other command is GPU-free. `WGPU_BACKEND=vulkan|dx12|metal|gl`
selects the backend.

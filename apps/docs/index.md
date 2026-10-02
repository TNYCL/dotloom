---
layout: home
hero:
  name: Dotloom
  text: Constraint-driven 2D editors
  tagline: A Rust engine for geometry, documents, rules and `.dotl` files — running headless or in the browser with a wgpu renderer, a framework-agnostic TypeScript SDK and an optional React editor.
  actions:
    - theme: brand
      text: Get started
      link: /guide/getting-started
    - theme: alt
      text: Open the playground
      link: /playground/
      target: _self
features:
  - title: Rules, not ad hoc code
    details: Equalities, inequalities, geometric relations and priorities are solved by the engine. Invalid edits are rejected with the rules involved and the nearest allowed value.
  - title: One document, many views
    details: The engine owns the only editable copy (in a Web Worker). Renderers and UIs receive revisioned deltas; undo, autosave and files all go through transactions.
  - title: WebGPU and WebGL2
    details: The same wgpu shaders run on both backends, validated separately in real browsers. No silent Canvas2D fallback.
  - title: Your objects
    details: Define entity types as data — properties, anchors, drawing recipes, rules, migrations — and add tools, panels and importers in TypeScript.
  - title: Real files
    details: "`.dotl` project files (ZIP) round-trip exactly; SVG, DXF and PNG import/export report what they cannot carry."
  - title: Open source
    details: MIT OR Apache-2.0. Rust crates, npm packages, CLI, docs and examples in one repository.
---

# ADR-0006: Plugin model

Status: accepted (2026-10-02)

## Decision

Two layers:

1. **Model definitions (serializable, evaluated in Rust).** An `EntityTypeDef` has a
   namespaced `typeId` (`vendor.name`), `version`, a typed property schema (number with
   dimension, bool, string, enum, point, entity reference), anchors and primitive
   recipes written in a typed expression AST (arithmetic, comparisons,
   `min/max/abs/sqrt/sin/cos/atan2/hypot`, vector ops, references to properties and
   referenced entities' anchors), constraint templates and migrations (renames,
   defaults, expressions). Expressions are dimension-checked at registration. There is
   no `eval`, no scripts and no JavaScript callbacks during solving. Unsupported
   functions or equation classes produce typed errors.
2. **Host code (trusted TypeScript).** Tools, commands, snap providers, inspector
   panels, import/export and storage adapters are TypeScript modules registered with
   the SDK. They translate interaction into engine commands.

The same definition JSON drives native (CLI/headless) and browser behaviour. A new
numeric backend or a new primitive kind requires a Rust extension and a rebuild.

Lifecycle: `register → enable → disable → dispose`. Duplicate `typeId` or
incompatible versions are errors. Disabling a plugin used by the open document makes
its entities read-only (payload kept, fallback representation shown) until re-enabled.

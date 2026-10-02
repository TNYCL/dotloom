/**
 * Public data types of the Dotloom SDK.
 *
 * They mirror the JSON produced and accepted by the Rust engine (serde,
 * camelCase). Numbers are in canonical units: millimetres, radians, seconds.
 */

/** `[x, y]` in model units (mm, Y up). */
export type Point = [number, number]
/** SVG-convention affine transform `[a, b, c, d, e, f]`. */
export type Affine = [number, number, number, number, number, number]

/** Axis-aligned bounding box. */
export interface Aabb {
  min: Point
  max: Point
}

export type HAlign = 'left' | 'center' | 'right'
export type VAlign = 'baseline' | 'middle' | 'top' | 'bottom'

export interface Text {
  position: Point
  content: string
  height: number
  rotation?: number
  halign?: HAlign
  valign?: VAlign
}

export type PathEl = { M: Point } | { L: Point } | { Q: [Point, Point] } | { C: [Point, Point, Point] } | 'Z'

/** Canonical geometry of built-in entities. */
export type Shape =
  | { type: 'point'; at: Point }
  | { type: 'line'; a: Point; b: Point }
  | { type: 'polyline'; points: Point[]; bulges?: number[]; closed?: boolean }
  | { type: 'rect'; origin: Point; width: number; height: number }
  | { type: 'circle'; center: Point; radius: number }
  | { type: 'arc'; center: Point; radius: number; start: number; sweep: number }
  | { type: 'path'; elements: PathEl[] }
  | { type: 'polygon'; outer: Point[]; holes?: Point[][] }
  | ({ type: 'text' } & Text)

export type EntityId = number
export type ConstraintId = number
export type LayerId = number
export type GroupId = number

/** Reference to another entity. */
export interface RefValue {
  ref: EntityId
}

/** Named anchor of an entity (`start`, `end`, `mid`, `center`, `v3`, plugin-defined). */
export interface AnchorRef {
  entity: EntityId
  anchor: string
}

/** Typed property value. */
export type PropValue = boolean | number | string | Point | RefValue | AnchorRef

/** `#rrggbb` or `#rrggbbaa`. */
export type Color = string

export interface Style {
  stroke?: Color
  fill?: Color
  strokeWidth?: number
  dash?: number[]
}

export interface Entity {
  id: EntityId
  type: string
  typeVersion?: number
  layer: LayerId
  transform?: Affine
  geometry?: Shape
  props?: Record<string, PropValue>
  data?: Record<string, unknown>
  name?: string
  style?: Style
  locked?: boolean
  hidden?: boolean
  fallback?: Shape[]
}

export interface Layer {
  id: LayerId
  name: string
  visible?: boolean
  locked?: boolean
  color?: Color
}

export interface Group {
  id: GroupId
  name?: string
  members: EntityId[]
  children?: GroupId[]
}

export type Cmp = '=' | '<=' | '>='

/** Numeric parameter of an entity. */
export type ParamRef = { entity: EntityId; prop: string } | { entity: EntityId; geom: string }

export interface LineRef {
  from: AnchorRef
  to: AnchorRef
}

export interface Term {
  coef: number
  param: ParamRef
}

/** Constraint kinds. */
export type RuleSpec =
  | { kind: 'fix'; param: ParamRef; value: number }
  | { kind: 'equal'; a: ParamRef; b: ParamRef }
  | { kind: 'allEqual'; params: ParamRef[] }
  | { kind: 'linear'; terms: Term[]; op: Cmp; rhs: number }
  | { kind: 'ratio'; a: ParamRef; b: ParamRef; k: number }
  | { kind: 'equalSpacing'; params: ParamRef[] }
  | { kind: 'coincident'; a: AnchorRef; b: AnchorRef }
  | { kind: 'horizontal'; a: AnchorRef; b: AnchorRef }
  | { kind: 'vertical'; a: AnchorRef; b: AnchorRef }
  | { kind: 'fixPoint'; a: AnchorRef; at: Point }
  | { kind: 'distance'; a: AnchorRef; b: AnchorRef; value: number }
  | { kind: 'pointLineDistance'; point: AnchorRef; line: LineRef; value: number }
  | { kind: 'pointOnLine'; point: AnchorRef; line: LineRef }
  | { kind: 'pointOnCircle'; point: AnchorRef; circle: EntityId }
  | { kind: 'length'; line: LineRef; value: number }
  | { kind: 'equalLength'; a: LineRef; b: LineRef }
  | { kind: 'parallel'; a: LineRef; b: LineRef }
  | { kind: 'perpendicular'; a: LineRef; b: LineRef }
  | { kind: 'angle'; a: LineRef; b: LineRef; value: number }
  | { kind: 'concentric'; a: EntityId; b: EntityId }
  | { kind: 'radius'; circle: EntityId; value: number }
  | { kind: 'equalRadius'; a: EntityId; b: EntityId }
  | { kind: 'tangentLineCircle'; line: LineRef; circle: EntityId; side: 1 | -1 }
  | { kind: 'tangentCircles'; a: EntityId; b: EntityId; internal?: boolean; sign?: 1 | -1 }
  | { kind: 'expression'; entity: EntityId; lhs: string; op: Cmp; rhs: string }

export type Strength = 'required' | 'strong' | 'medium' | 'weak'

export interface Constraint {
  id: ConstraintId
  rule: RuleSpec
  strength?: Strength
  enabled?: boolean
  label?: string
  source?: string
  owner?: EntityId
}

export type LengthUnit = 'millimetre' | 'centimetre' | 'metre' | 'inch' | 'foot'

export interface TimeAxis {
  origin_s: number
  mm_per_second: number
}

export interface Settings {
  displayUnit?: LengthUnit
  grid?: { spacing: number; majorEvery: number }
  timeAxis?: TimeAxis
}

/** A document as stored in `document.json`. */
export interface DocumentJson {
  schema: number
  meta?: { title?: string }
  settings?: Settings
  layers: Layer[]
  entities: Entity[]
  groups: Group[]
  constraints: Constraint[]
  nextId: number
}

// ---------------------------------------------------------------------------
// Commands

export type EditMode = 'exact' | 'prefer'
export type TransformPolicy = 'strict' | 'convert'
export type DeletePolicy = 'cascade' | 'reject'
export type CurveEnd = 'start' | 'end'

export interface NewEntity {
  type?: string
  layer?: LayerId
  geometry?: Shape
  props?: Record<string, PropValue>
  transform?: Affine
  name?: string
  style?: Style
  data?: Record<string, unknown>
}

export interface EntityPatch {
  geometry?: Shape
  props?: Record<string, PropValue | null>
  transform?: Affine
  name?: string
  style?: Style
  layer?: LayerId
  locked?: boolean
  hidden?: boolean
  data?: Record<string, unknown | null>
}

export interface ParamValue {
  entity: EntityId
  param: string
  value: number
}

export interface ConstraintSpec {
  rule: RuleSpec
  strength?: Strength
  enabled?: boolean
  label?: string
  source?: string
}

export interface Clipboard {
  entities: Entity[]
  constraints: Constraint[]
  groups: Group[]
}

export type Command =
  | { op: 'createEntity'; id?: EntityId; entity: NewEntity }
  | { op: 'updateEntity'; id: EntityId; patch: EntityPatch }
  | { op: 'setParams'; values: ParamValue[]; mode?: EditMode }
  | { op: 'transform'; ids: EntityId[]; transform: Affine; policy?: TransformPolicy }
  | { op: 'delete'; ids: EntityId[]; policy?: DeletePolicy }
  | { op: 'reorder'; id: EntityId; index: number }
  | { op: 'addConstraint'; id?: ConstraintId; constraint: ConstraintSpec }
  | {
      op: 'updateConstraint'
      id: ConstraintId
      patch: { rule?: RuleSpec; strength?: Strength; enabled?: boolean; label?: string }
    }
  | { op: 'removeConstraint'; id: ConstraintId }
  | { op: 'addLayer'; id?: LayerId; name: string }
  | { op: 'updateLayer'; id: LayerId; patch: { name?: string; visible?: boolean; locked?: boolean; color?: Color } }
  | { op: 'removeLayer'; id: LayerId }
  | { op: 'moveLayer'; id: LayerId; index: number }
  | { op: 'group'; id?: GroupId; members: EntityId[]; name?: string }
  | { op: 'ungroup'; id: GroupId }
  | { op: 'paste'; clipboard: Clipboard; offset: Point }
  | { op: 'split'; id: EntityId; at: Point; at2?: Point }
  | { op: 'trim'; id: EntityId; cutters: EntityId[]; pick: Point }
  | { op: 'extend'; id: EntityId; end: CurveEnd; boundaries: EntityId[] }
  | {
      op: 'setSettings'
      patch: { displayUnit?: LengthUnit; gridSpacing?: number; timeAxis?: TimeAxis; title?: string }
    }

export interface Transaction {
  label?: string
  commands: Command[]
}

// ---------------------------------------------------------------------------
// Results and diagnostics

export type SolveStatus =
  | { status: 'solved' }
  | { status: 'underconstrained'; dof: number }
  | { status: 'conflicting' }
  | { status: 'notConverged'; suspected_conflict: boolean }
  | { status: 'cancelled' }
  | { status: 'unsupported' }

export type DiagnosticKind =
  | 'conflict'
  | 'suspectedConflict'
  | 'notConverged'
  | 'redundant'
  | 'unsupported'
  | 'preferenceUnmet'
  | 'cancelled'

export interface DiagnosticReport {
  kind: DiagnosticKind
  certainty: 'certain' | 'suspected'
  constraints: ConstraintId[]
  templates: [EntityId, string][]
  edits: [EntityId, string][]
  entities: EntityId[]
  labels: string[]
  residual: number | null
  message: string
}

export interface NearestValue {
  entity: EntityId
  param: string
  requested: number
  feasible: number
}

export interface CommitReport {
  revision: number
  status: SolveStatus
  diagnostics: DiagnosticReport[]
  created: EntityId[]
  createdConstraints: ConstraintId[]
  deleted: EntityId[]
  removedConstraints: ConstraintId[]
  changed: EntityId[]
  notes: string[]
  undoAvailable: boolean
  /** Solver statistics when rules were solved. */
  solver?: SolverStats
}

export interface SolverStats {
  iterations: number
  attempts: number
  variables: number
  rules: number
  components: number
}

export interface Hit {
  entity: EntityId
  distance: number
}

export type SnapKind = 'endpoint' | 'intersection' | 'center' | 'midpoint' | 'quadrant' | 'anchor' | 'nearest' | 'grid'

export interface Snap {
  point: Point
  kind: SnapKind
  entity: EntityId | null
  anchor: string | null
  other: EntityId | null
}

export interface SnapOptions {
  endpoint?: boolean
  midpoint?: boolean
  center?: boolean
  quadrant?: boolean
  intersection?: boolean
  anchor?: boolean
  nearest?: boolean
  grid?: boolean
  gridSpacing?: number
}

export interface SnapQuery {
  point: Point
  /** Radius in model units (convert screen pixels with the camera). */
  radius: number
  options?: SnapOptions
  exclude?: EntityId[]
  previous?: Snap | null
}

export type DragSpec =
  | { kind: 'move'; ids: EntityId[]; from: Point }
  | { kind: 'anchor'; entity: EntityId; anchor: string }

export interface DragPreview {
  accepted: boolean
  status: SolveStatus
  diagnostics: DiagnosticReport[]
  /** The preview was re-solved incrementally (linear session) instead of from scratch. */
  incremental: boolean
}

export type AnchorKind =
  | 'endpoint'
  | 'midpoint'
  | 'center'
  | 'vertex'
  | 'quadrant'
  | 'corner'
  | 'insert'
  | 'centroid'
  | 'node'

export interface Anchor {
  name: string
  kind: AnchorKind
  point: Point
}

export interface ReadOnlyReason {
  reason: 'missingPlugin' | 'disabledPlugin' | 'newerVersion'
  type_id: string
  found?: number
  supported?: number
}

export interface EntityInfo {
  entity: Entity
  anchors: Anchor[]
  bbox: Aabb
  params: Record<string, number>
  readOnly: ReadOnlyReason | null
  error: string | null
  measured: number | null
  constraints: ConstraintId[]
}

export type LossKind =
  | 'constraints'
  | 'pluginGeometry'
  | 'dimensions'
  | 'approximated'
  | 'unsupported'
  | 'style'
  | 'text'
  | 'externalReference'
  | 'units'
  | 'fallback'

export interface ConversionReport {
  entities: number
  losses: { kind: LossKind; what: string; count: number }[]
  notes: string[]
}

export interface LoadReport {
  revision: number
  migrations: { from: number; message: string }[]
  plugins: { typeId: string; version: number }[]
  missingPlugins: string[]
  warnings: string[]
  view: unknown
}

export interface Capabilities {
  engineVersion: string
  protocol: number
  schema: number
  formatVersion: number
  sceneFormat: number
  import: string[]
  export: string[]
}

/** Engine events (after commits, never during). */
export type EngineEvent =
  | {
      type: 'committed'
      revision: number
      label: string
      cause: string
      entities: EntityId[]
      constraints: ConstraintId[]
    }
  | { type: 'selectionChanged'; selection: EntityId[] }
  | {
      type: 'historyChanged'
      canUndo: boolean
      canRedo: boolean
      undoLabel: string | null
      redoLabel: string | null
    }
  | { type: 'pluginsChanged' }

// ---------------------------------------------------------------------------
// Plugin definitions (data, evaluated in Rust; see ADR-0006)

export type DimName = 'length' | 'angle' | 'time' | 'scalar'
export type NumberLit = number | string
export type StayLevel = 'low' | 'normal' | 'high'

export type PropDef =
  | {
      type: 'number'
      dim?: DimName
      default?: NumberLit
      min?: NumberLit
      max?: NumberLit
      solve?: boolean
      stay?: StayLevel
      label?: string
    }
  | { type: 'point'; default?: Point; stay?: StayLevel; label?: string }
  | { type: 'bool'; default?: boolean; label?: string }
  | { type: 'text'; default?: string; maxLen?: number; label?: string }
  | { type: 'enum'; values: string[]; default?: string; label?: string }
  | { type: 'ref'; target?: string; onDelete?: 'cascade' | 'clear' | 'reject'; required?: boolean; label?: string }

export interface PrimStyle {
  stroke?: Color
  fill?: Color
  width?: number
  dash?: number[]
  noStroke?: boolean
}

export type PrimitiveDef =
  | { kind: 'line'; from: string; to: string; style?: PrimStyle }
  | { kind: 'polyline'; points: string[]; closed?: boolean; style?: PrimStyle }
  | { kind: 'polygon'; points: string[]; style?: PrimStyle }
  | { kind: 'circle'; center: string; radius: string; style?: PrimStyle }
  | { kind: 'arc'; center: string; radius: string; start: string; sweep: string; style?: PrimStyle }
  | {
      kind: 'text'
      position: string
      content: string
      height: string
      rotation?: string
      halign?: HAlign
      valign?: VAlign
      style?: PrimStyle
    }

/** A plugin entity type (serializable; no executable code). */
export interface EntityTypeDef {
  typeId: string
  version?: number
  label?: string
  props?: Record<string, PropDef>
  derived?: { name: string; expr: string }[]
  anchors?: { name: string; expr: string; kind?: AnchorKind }[]
  primitives?: PrimitiveDef[]
  constraints?: { lhs: string; op: Cmp; rhs: string; label?: string; strength?: Strength }[]
  circle?: { center: string; radius: string }
  migrations?: { from: number; rename?: Record<string, string>; set?: Record<string, PropValue>; remove?: string[] }[]
}

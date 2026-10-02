/**
 * `@dotloom/sdk` — framework-agnostic TypeScript SDK for Dotloom.
 *
 * Importing this module has no side effects and touches no browser globals, so it
 * is safe in SSR/build environments; browser APIs are used only when an engine,
 * renderer or editor is created.
 */

export {
  type Camera,
  DEFAULT_CAMERA,
  fitBounds,
  isValidCamera,
  MAX_SCALE,
  MIN_SCALE,
  panBy,
  pxToWorld,
  screenToWorld,
  type ViewSize,
  visibleBounds,
  wheelZoomFactor,
  worldToScreen,
  zoomAt,
} from './camera.js'
export { formatColor, parseColor, type ThemeColors, type ThemeInput, type ThemeName } from './color.js'
export * from './editor/index.js'
export {
  type CallOptions,
  DotloomEngine,
  defaultEngineWasmUrl,
  type EngineEventName,
  type EngineOptions,
  type SceneMessage,
} from './engine.js'
export { DEFAULT_STEP_BUDGET, type EngineFactory, EngineHost } from './host.js'
export {
  type ConstraintTemplate,
  type DotloomPlugin,
  type Exporter,
  type Importer,
  type PanelContribution,
  type PluginApi,
  type PluginCommand,
  PluginHost,
  type PluginInfo,
  type SnapCandidate,
  type SnapProvider,
} from './plugins.js'
export {
  type ClientMessage,
  DotloomError,
  type ErrorCode,
  type ErrorPayload,
  type HostMessage,
  type MethodName,
  PROTOCOL_VERSION,
} from './protocol.js'
export {
  decodeSceneDelta,
  isEmptyDelta,
  type Primitive,
  SCENE_FORMAT_VERSION,
  type SceneDelta,
  SceneFlags,
  type SceneItem,
  SceneStore,
  type Stroke,
} from './scene.js'
export {
  Autosave,
  type AutosaveOptions,
  type AutosaveState,
  DOTL_MIME,
  downloadBytes,
  type FileKind,
  fileKind,
  IndexedDbStorage,
  MemoryStorage,
  type OpenedFile,
  pickFile,
  readFile,
  type StorageAdapter,
  type StoredFile,
  type StoredMeta,
} from './storage.js'
export { InlineTransport, type Transport, WorkerTransport } from './transport.js'
export type * from './types.js'
export { formatAngle, formatLength, LENGTH_UNITS, parseAngle, parseLength, type UnitInfo } from './units.js'
export { SDK_VERSION, satisfies } from './version.js'
export {
  type Backend,
  type BackendAttempt,
  DEFAULT_GRID,
  type FrameStats,
  type GridSettings,
  loadRenderer,
  type MarkerKind,
  type OverlayState,
  RENDER_PROTOCOL,
  type RendererInfo,
  Viewport,
  type ViewportLike,
  type ViewportOptions,
} from './viewport.js'

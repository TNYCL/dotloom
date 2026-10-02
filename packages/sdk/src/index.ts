/**
 * `@dotloom/sdk` — framework-agnostic TypeScript SDK for Dotloom.
 *
 * Importing this module has no side effects and touches no browser globals, so it
 * is safe in SSR/build environments; browser APIs are used only when an engine,
 * renderer or editor is created.
 */

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
export { InlineTransport, type Transport, WorkerTransport } from './transport.js'
export type * from './types.js'

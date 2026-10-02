/// <reference lib="webworker" />
/**
 * Web Worker entry: hosts the engine WASM. Created by `WorkerTransport` with
 * `new Worker(new URL('./worker.js', import.meta.url), { type: 'module' })`.
 */

import { EngineHost } from './host.js'
import type { ClientMessage } from './protocol.js'
import init, { WasmEngine } from './wasm/engine/dotloom_wasm.js'

const scope = self as unknown as DedicatedWorkerGlobalScope

const host = new EngineHost(
  (msg, transfer) => scope.postMessage(msg, transfer ?? []),
  async (engineWasm) => {
    await init(engineWasm ? { module_or_path: engineWasm } : undefined)
    return WasmEngine
  },
)

scope.onmessage = (ev: MessageEvent<ClientMessage>) => host.receive(ev.data)

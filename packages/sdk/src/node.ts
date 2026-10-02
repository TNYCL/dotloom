/**
 * Node.js entry: runs the engine in the calling thread (no Worker), loading the
 * WASM module from the installed package. Use it for tests, CLIs and servers.
 *
 * ```ts
 * import { createNodeEngine } from '@dotloom/sdk/node'
 * const engine = await createNodeEngine()
 * ```
 */

import { readFile } from 'node:fs/promises'
import { DotloomEngine, type EngineOptions } from './engine.js'
import { InlineTransport } from './transport.js'
import { initSync, WasmEngine } from './wasm/engine/dotloom_wasm.js'

let initialized = false

/** Create an in-thread engine for Node.js. */
export async function createNodeEngine(
  options: Omit<EngineOptions, 'transport' | 'workerUrl'> & { stepBudget?: number } = {},
): Promise<DotloomEngine> {
  const transport = new InlineTransport(async (url) => {
    if (!initialized) {
      const source = url ?? options.engineWasmUrl ?? new URL('./wasm/engine/dotloom_wasm_bg.wasm', import.meta.url)
      const bytes = await readFile(typeof source === 'string' && !source.startsWith('file:') ? source : new URL(source))
      initSync({ module: bytes })
      initialized = true
    }
    return WasmEngine
  }, options.stepBudget)
  const opts: EngineOptions = { transport }
  if (options.requestTimeoutMs !== undefined) opts.requestTimeoutMs = options.requestTimeoutMs
  return DotloomEngine.create(opts)
}

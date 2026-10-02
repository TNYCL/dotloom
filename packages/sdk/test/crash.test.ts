/**
 * Engine crash reporting (separate file: a trapped WASM instance is not reused).
 */

import { describe, expect, it } from 'vitest'
import { DotloomError } from '../src/index.js'
import { createNodeEngine } from '../src/node.js'

describe('engine crash', () => {
  it('rejects pending and later calls with `crashed` and emits a crash event', async () => {
    const e = await createNodeEngine()
    await e.apply([{ op: 'createEntity', entity: { geometry: { type: 'point', at: [1, 2] } } }])
    const events: string[] = []
    e.on('crash', (c) => events.push(c.message))
    const queued = e.historyState()
    const err = await e.debugCrashForTesting().catch((x) => x)
    expect(err).toBeInstanceOf(DotloomError)
    expect(err.code).toBe('crashed')
    await expect(queued).resolves.toBeTruthy()
    await expect(e.undo()).rejects.toMatchObject({ code: 'crashed' })
    expect(e.isCrashed).toBe(true)
    expect(events).toHaveLength(1)
    e.dispose()
  })
})

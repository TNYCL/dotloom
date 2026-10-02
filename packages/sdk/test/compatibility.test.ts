/**
 * The compatibility manifest (compatibility.json) matches the TypeScript side:
 * package versions (lockstep with the crates), worker protocol, renderer binding
 * protocol and scene format. The Rust side is checked by
 * crates/wasm/tests/compatibility.rs.
 */

import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'
import { PROTOCOL_VERSION } from '../src/protocol.js'
import { SCENE_FORMAT_VERSION } from '../src/scene.js'
import { SDK_VERSION } from '../src/version.js'
import { RENDER_PROTOCOL } from '../src/viewport.js'

const root = resolve(process.cwd(), '../..')
const json = (rel: string) => JSON.parse(readFileSync(resolve(root, rel), 'utf8')) as Record<string, unknown>

describe('compatibility manifest', () => {
  it('matches the SDK, the React package and the protocol constants', () => {
    const m = json('compatibility.json') as {
      release: string
      packages: Record<string, string>
      formats: { workerProtocol: number; renderProtocol: number; sceneFormat: { version: number } }
      toolchains: { react: string }
    }
    const sdk = json('packages/sdk/package.json') as { name: string; version: string }
    const react = json('packages/react/package.json') as {
      name: string
      version: string
      peerDependencies: Record<string, string>
    }
    expect(SDK_VERSION).toBe(m.release)
    expect(sdk.version).toBe(m.packages[sdk.name])
    expect(react.version).toBe(m.packages[react.name])
    expect(sdk.version).toBe(m.packages.crates)
    expect(PROTOCOL_VERSION).toBe(m.formats.workerProtocol)
    expect(RENDER_PROTOCOL).toBe(m.formats.renderProtocol)
    expect(SCENE_FORMAT_VERSION).toBe(m.formats.sceneFormat.version)
    expect(react.peerDependencies.react).toBe(`${m.toolchains.react}.0.0`)
  })
})

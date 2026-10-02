// Runs against the installed packages (node --test). No workspace sources.

import assert from 'node:assert/strict'
import { test } from 'node:test'
import { builtinTools, EditorCore, NullViewport, PluginHost } from '@dotloom/sdk'
import { createNodeEngine } from '@dotloom/sdk/node'
import { acmePlugin } from '../src/plugin.js'

async function setup() {
  const engine = await createNodeEngine()
  const core = new EditorCore(engine, new NullViewport())
  for (const t of builtinTools()) core.registerTool(t)
  core.start()
  const host = new PluginHost(engine, core)
  await host.register(acmePlugin)
  return { engine, core, host }
}

test('the plugin type follows its rule: width grows with the seat count', async () => {
  const { engine, core } = await setup()
  const r = await engine.apply([{ op: 'createEntity', entity: { type: 'acme.table', props: { seats: 6 } } }])
  const id = r.created[0]
  const info = await engine.entityInfo(id)
  assert.ok(info.params.width >= 1800 - 1e-6, `width ${info.params.width}`)
  // A table that is too narrow for its seats is rejected with the rule's label.
  const err = await engine
    .apply([{ op: 'setParams', values: [{ entity: id, param: 'width', value: 1000 }] }])
    .catch((e) => e)
  assert.equal(err.code, 'solve')
  assert.match(JSON.stringify(err.details), /every seat gets 60 cm/)
  core.dispose()
  engine.dispose()
})

test('the plugin tool places tables and the document round-trips through .dotl', async () => {
  const { engine, core } = await setup()
  core.setTool('acme.table')
  const vp = core.viewport
  const [x, y] = vp.worldToScreen([500, 500])
  await core.pointerDown({ x, y, button: 0, buttons: 1, shift: false, mod: false, alt: true, pointerId: 1, pointerType: 'mouse' })
  await core.idle()
  const doc = await engine.documentJson()
  assert.equal(doc.entities.filter((e) => e.type === 'acme.table').length, 1)
  const bytes = await engine.save()
  const e2 = await createNodeEngine()
  const report = await e2.load(bytes)
  assert.deepEqual(report.missingPlugins, ['acme.table'])
  const info = await e2.entityInfo(doc.entities[0].id)
  assert.equal(info.readOnly?.reason, 'missingPlugin')
  core.dispose()
  engine.dispose()
  e2.dispose()
})

test('disabling the plugin makes its objects read-only and removes its tool', async () => {
  const { engine, core, host } = await setup()
  const r = await engine.apply([{ op: 'createEntity', entity: { type: 'acme.table' } }])
  await host.disable('acme.furniture')
  assert.equal((await engine.entityInfo(r.created[0])).readOnly?.reason, 'disabledPlugin')
  assert.ok(!core.listTools().some((t) => t.id === 'acme.table'))
  await host.enable('acme.furniture')
  assert.equal((await engine.entityInfo(r.created[0])).readOnly, null)
  core.dispose()
  engine.dispose()
})

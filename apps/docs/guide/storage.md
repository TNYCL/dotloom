# Storage and files

## Files in the browser

```ts
import { pickFile, downloadBytes, DOTL_MIME } from '@dotloom/sdk'

const f = await pickFile('.dotl,.svg,.dxf')       // null when cancelled
if (f?.kind === 'dotl') await engine.load(f.bytes)
downloadBytes(await engine.save({ camera }), 'ev-plani.dotl', DOTL_MIME)
```

`load` rejects corrupt or oversized files with typed errors (`file`) and never
changes the current document in that case. SVG and DXF imports are one undoable
transaction and return a loss report.

## Autosave and recovery

```ts
import { Autosave, IndexedDbStorage } from '@dotloom/sdk'

const autosave = new Autosave(engine, new IndexedDbStorage(), { name: 'raf-tasarimi.dotl' })
autosave.start()                       // saves shortly after each commit and when the tab hides
const rec = await autosave.recoverable()  // unsaved work from a previous session?
if (rec) await autosave.restore()
await autosave.markSaved()             // after the user saved to a file
```

`autosave.state` exposes `pending`, `saving`, `dirty`, `lastSavedAt` and `error`
for status displays.

## Your own storage

Implement `StorageAdapter` (`read`, `meta`, `write`, `remove`, `list`) to store
documents in a server, a cloud drive or OPFS; plugins can contribute adapters too.
`MemoryStorage` is included for tests and previews.

## Native saving

`dotloom-io` writes `.dotl` files atomically: a temporary file in the target
directory is written and flushed, then renamed over the target. If anything fails,
the previous file is untouched.

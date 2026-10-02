/**
 * Storage: an async adapter interface hosts can implement, IndexedDB and in-memory
 * adapters, browser file open/save helpers, and autosave with crash recovery.
 *
 * Only `.dotl` bytes produced by the engine are stored; nothing executable.
 */

import { Store } from './editor/store.js'
import type { DotloomEngine } from './engine.js'
import { DotloomError } from './protocol.js'

export interface StoredMeta {
  /** Display name (e.g. `raf-tasarimi.dotl`). */
  name: string
  /** Milliseconds since the epoch. */
  savedAt: number
  /** Engine revision at save time. */
  revision: number
  size: number
  /** `false` when the stored copy has changes not yet saved by the user. */
  clean: boolean
}

export interface StoredFile extends StoredMeta {
  key: string
}

/** Pluggable async storage (IndexedDB, OPFS, a server, cloud drives, …). */
export interface StorageAdapter {
  readonly id: string
  read(key: string): Promise<Uint8Array | null>
  meta(key: string): Promise<StoredMeta | null>
  write(key: string, bytes: Uint8Array, meta: StoredMeta): Promise<void>
  remove(key: string): Promise<void>
  list(): Promise<StoredFile[]>
}

/** In-memory storage (tests, SSR, previews). */
export class MemoryStorage implements StorageAdapter {
  readonly id = 'memory'
  private readonly files = new Map<string, { bytes: Uint8Array; meta: StoredMeta }>()

  async read(key: string): Promise<Uint8Array | null> {
    const f = this.files.get(key)
    return f ? f.bytes.slice() : null
  }

  async meta(key: string): Promise<StoredMeta | null> {
    const f = this.files.get(key)
    return f ? { ...f.meta } : null
  }

  async write(key: string, bytes: Uint8Array, meta: StoredMeta): Promise<void> {
    this.files.set(key, { bytes: bytes.slice(), meta: { ...meta } })
  }

  async remove(key: string): Promise<void> {
    this.files.delete(key)
  }

  async list(): Promise<StoredFile[]> {
    return [...this.files.entries()].map(([key, f]) => ({ key, ...f.meta }))
  }
}

function req<T>(r: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    r.onsuccess = () => resolve(r.result)
    r.onerror = () => reject(r.error ?? new Error('IndexedDB request failed'))
  })
}

/** IndexedDB storage (browser). Data and metadata live in separate stores. */
export class IndexedDbStorage implements StorageAdapter {
  readonly id = 'indexeddb'
  private db: Promise<IDBDatabase> | null = null

  constructor(private readonly dbName = 'dotloom') {}

  private open(): Promise<IDBDatabase> {
    if (typeof indexedDB === 'undefined') {
      return Promise.reject(new DotloomError({ code: 'file', message: 'IndexedDB is not available' }))
    }
    this.db ??= new Promise<IDBDatabase>((resolve, reject) => {
      const r = indexedDB.open(this.dbName, 1)
      r.onupgradeneeded = () => {
        r.result.createObjectStore('files')
        r.result.createObjectStore('meta')
      }
      r.onsuccess = () => resolve(r.result)
      r.onerror = () => reject(r.error ?? new Error('cannot open IndexedDB'))
      r.onblocked = () => reject(new Error('IndexedDB upgrade blocked by another tab'))
    })
    this.db.catch(() => {
      this.db = null
    })
    return this.db
  }

  async read(key: string): Promise<Uint8Array | null> {
    const db = await this.open()
    const v = (await req(db.transaction('files').objectStore('files').get(key))) as ArrayBuffer | undefined
    return v ? new Uint8Array(v) : null
  }

  async meta(key: string): Promise<StoredMeta | null> {
    const db = await this.open()
    return ((await req(db.transaction('meta').objectStore('meta').get(key))) as StoredMeta | undefined) ?? null
  }

  async write(key: string, bytes: Uint8Array, meta: StoredMeta): Promise<void> {
    const db = await this.open()
    const tx = db.transaction(['files', 'meta'], 'readwrite')
    tx.objectStore('files').put(bytes.slice().buffer, key)
    tx.objectStore('meta').put(meta, key)
    await new Promise<void>((resolve, reject) => {
      tx.oncomplete = () => resolve()
      tx.onerror = () => reject(tx.error ?? new Error('IndexedDB write failed'))
      tx.onabort = () => reject(tx.error ?? new Error('IndexedDB write aborted (quota?)'))
    })
  }

  async remove(key: string): Promise<void> {
    const db = await this.open()
    const tx = db.transaction(['files', 'meta'], 'readwrite')
    tx.objectStore('files').delete(key)
    tx.objectStore('meta').delete(key)
    await new Promise<void>((resolve, reject) => {
      tx.oncomplete = () => resolve()
      tx.onerror = () => reject(tx.error ?? new Error('IndexedDB delete failed'))
    })
  }

  async list(): Promise<StoredFile[]> {
    const db = await this.open()
    const store = db.transaction('meta').objectStore('meta')
    const keys = (await req(store.getAllKeys())) as string[]
    const metas = (await req(store.getAll())) as StoredMeta[]
    return keys.map((key, i) => ({ key, ...(metas[i] as StoredMeta) }))
  }

  /** Close the database connection. */
  close(): void {
    void this.db?.then((d) => d.close()).catch(() => undefined)
    this.db = null
  }
}

// ---------------------------------------------------------------------------
// Browser files

export type FileKind = 'dotl' | 'svg' | 'dxf' | 'unknown'

export interface OpenedFile {
  name: string
  kind: FileKind
  bytes: Uint8Array
}

/** File kind from a name. */
export function fileKind(name: string): FileKind {
  const ext = name.toLowerCase().split('.').pop() ?? ''
  return ext === 'dotl' || ext === 'svg' || ext === 'dxf' ? ext : 'unknown'
}

/** Read a `File`/`Blob` picked by the user. */
export async function readFile(file: File): Promise<OpenedFile> {
  return { name: file.name, kind: fileKind(file.name), bytes: new Uint8Array(await file.arrayBuffer()) }
}

/** Ask the user for a file (resolves `null` when cancelled). */
export function pickFile(accept = '.dotl,.svg,.dxf'): Promise<OpenedFile | null> {
  return new Promise((resolve, reject) => {
    const input = document.createElement('input')
    input.type = 'file'
    input.accept = accept
    input.style.display = 'none'
    input.addEventListener('change', () => {
      const f = input.files?.[0]
      input.remove()
      if (!f) resolve(null)
      else readFile(f).then(resolve, reject)
    })
    input.addEventListener('cancel', () => {
      input.remove()
      resolve(null)
    })
    document.body.appendChild(input)
    input.click()
  })
}

/** Offer bytes as a download. */
export function downloadBytes(bytes: Uint8Array, name: string, mime = 'application/octet-stream'): void {
  const blob = new Blob([bytes.slice().buffer], { type: mime })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = name
  a.style.display = 'none'
  document.body.appendChild(a)
  a.click()
  a.remove()
  setTimeout(() => URL.revokeObjectURL(url), 1000)
}

/** MIME type registered for `.dotl` downloads. */
export const DOTL_MIME = 'application/vnd.dotloom+zip'

// ---------------------------------------------------------------------------
// Autosave

export interface AutosaveOptions {
  /** Storage key of the autosave slot. */
  key?: string
  /** Delay after the last commit before saving. */
  debounceMs?: number
  /** Document name stored with the autosave. */
  name?: string
  /** Called to include view state (camera, panels) in the autosave. */
  view?: () => unknown
}

export interface AutosaveState {
  /** A save is scheduled or running (the stored copy is behind). */
  pending: boolean
  saving: boolean
  lastSavedAt: number | null
  /** Unsaved-to-file changes exist (autosave holds them). */
  dirty: boolean
  error: string | null
}

/**
 * Saves the engine's document to a `StorageAdapter` shortly after each commit,
 * and offers recovery after a crash or a closed tab.
 */
export class Autosave {
  readonly state = new Store<AutosaveState>({
    pending: false,
    saving: false,
    lastSavedAt: null,
    dirty: false,
    error: null,
  })
  private timer: ReturnType<typeof setTimeout> | null = null
  private readonly key: string
  private readonly debounceMs: number
  private name: string
  private readonly off: (() => void)[] = []
  private chain: Promise<void> = Promise.resolve()
  private disposed = false

  constructor(
    private readonly engine: DotloomEngine,
    private readonly storage: StorageAdapter,
    private readonly options: AutosaveOptions = {},
  ) {
    this.key = options.key ?? 'autosave'
    this.debounceMs = options.debounceMs ?? 800
    this.name = options.name ?? 'untitled.dotl'
  }

  /** Start watching commits. */
  start(): void {
    this.off.push(
      this.engine.on('committed', (ev) => {
        if (ev.cause === 'load') return
        this.state.set({ dirty: true })
        this.schedule()
      }),
    )
    if (typeof document !== 'undefined') {
      // Best effort when the tab is hidden or closed (IndexedDB writes are async).
      const hide = (): void => {
        if (this.state.getSnapshot().pending) void this.flush()
      }
      const onVisibility = (): void => {
        if (document.visibilityState === 'hidden') hide()
      }
      document.addEventListener('visibilitychange', onVisibility)
      window.addEventListener('pagehide', hide)
      this.off.push(() => {
        document.removeEventListener('visibilitychange', onVisibility)
        window.removeEventListener('pagehide', hide)
      })
    }
  }

  setName(name: string): void {
    this.name = name
  }

  private schedule(): void {
    this.state.set({ pending: true })
    if (this.timer) clearTimeout(this.timer)
    this.timer = setTimeout(() => {
      this.timer = null
      void this.flush()
    }, this.debounceMs)
  }

  /** Save now (serialized with other saves). */
  flush(): Promise<void> {
    if (this.timer) {
      clearTimeout(this.timer)
      this.timer = null
    }
    this.chain = this.chain.then(async () => {
      if (this.disposed) return
      this.state.set({ saving: true })
      const revision = this.engine.revision
      try {
        const bytes = await this.engine.save(this.options.view?.())
        await this.storage.write(this.key, bytes, {
          name: this.name,
          savedAt: Date.now(),
          revision: this.engine.revision,
          size: bytes.byteLength,
          clean: !this.state.getSnapshot().dirty,
        })
        // Still pending if another commit arrived while saving.
        this.state.set({
          saving: false,
          pending: this.timer !== null || this.engine.revision !== revision,
          lastSavedAt: Date.now(),
          error: null,
        })
      } catch (e) {
        this.state.set({ saving: false, error: e instanceof Error ? e.message : String(e) })
      }
    })
    return this.chain
  }

  /** Call after the user saved the document elsewhere (e.g. downloaded the file). */
  async markSaved(): Promise<void> {
    this.state.set({ dirty: false })
    await this.flush()
  }

  /** An autosave with unsaved changes from a previous session, if any. */
  async recoverable(): Promise<StoredMeta | null> {
    const m = await this.storage.meta(this.key)
    return m && !m.clean ? m : null
  }

  /** Load the autosaved document into the engine. Returns the stored view state. */
  async restore(): Promise<unknown> {
    const bytes = await this.storage.read(this.key)
    if (!bytes) throw new DotloomError({ code: 'notFound', message: 'no autosave to restore' })
    const report = await this.engine.load(bytes)
    const meta = await this.storage.meta(this.key)
    if (meta) this.name = meta.name
    this.state.set({ dirty: true })
    return report.view
  }

  /** Forget the autosave (e.g. the user discarded recovery). */
  async discard(): Promise<void> {
    await this.storage.remove(this.key)
    this.state.set({ dirty: false })
  }

  dispose(): void {
    this.disposed = true
    if (this.timer) clearTimeout(this.timer)
    for (const f of this.off.splice(0)) f()
  }
}

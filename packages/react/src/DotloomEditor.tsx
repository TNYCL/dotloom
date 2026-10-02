/**
 * `<DotloomEditor>`: a complete reference editor. Every part is also exported
 * separately so hosts can compose their own layout around `EditorProvider`.
 */

import {
  Autosave,
  bindDom,
  builtinTools,
  type Camera,
  DotloomEngine,
  type DotloomPlugin,
  EditorCore,
  type EditorCoreOptions,
  type EngineOptions,
  IndexedDbStorage,
  isValidCamera,
  LENGTH_UNITS,
  type LengthUnit,
  MemoryStorage,
  NullViewport,
  PluginHost,
  type StorageAdapter,
  type StoredMeta,
  Viewport,
  type ViewportOptions,
} from '@dotloomjs/sdk'
import {
  type CSSProperties,
  type ReactNode,
  type RefObject,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { type Action, buildActions, type FileHooks } from './actions.js'
import { CommandPalette } from './components/CommandPalette.js'
import { ConstraintsPanel } from './components/ConstraintsPanel.js'
import { ErrorBanner, MissingPluginsBanner, RecoveryDialog, type UiError } from './components/Dialogs.js'
import { Inspector } from './components/Inspector.js'
import { Icon } from './components/icons.js'
import { LayersPanel } from './components/LayersPanel.js'
import { ObjectsPanel } from './components/ObjectsPanel.js'
import { PluginPanels } from './components/PluginPanels.js'
import { StatusBar } from './components/StatusBar.js'
import { Toolbar } from './components/Toolbar.js'
import { type EditorHandle, EditorProvider, useDocument, useEditor, useEditorState } from './context.js'
import { I18nProvider, useT } from './i18n.js'

export type ThemeMode = 'light' | 'dark' | 'system'

export interface DotloomEditorProps {
  plugins?: DotloomPlugin[]
  locale?: string
  messages?: Partial<Record<string, string>>
  theme?: ThemeMode
  /**
   * Autosave target: a custom adapter, else the first storage adapter contributed by
   * an enabled plugin, else IndexedDB (memory where IndexedDB is unavailable).
   * `false` disables autosave.
   */
  autosave?: boolean | { key?: string; storage?: StorageAdapter }
  /** Initial `.dotl` bytes. */
  document?: Uint8Array
  /** Document file name used when saving. */
  name?: string
  engine?: EngineOptions
  viewport?: Omit<ViewportOptions, 'theme'>
  core?: EditorCoreOptions
  onReady?: (handle: EditorHandle) => void
  /** Extra sidebar content (rendered inside the editor context). */
  children?: ReactNode
  className?: string
  style?: CSSProperties
}

function useResolvedTheme(mode: ThemeMode): 'light' | 'dark' {
  const query = typeof matchMedia === 'function' ? matchMedia('(prefers-color-scheme: dark)') : null
  const [dark, setDark] = useState(query?.matches ?? false)
  useEffect(() => {
    if (!query) return
    const on = (): void => setDark(query.matches)
    query.addEventListener('change', on)
    return () => query.removeEventListener('change', on)
  }, [query])
  return mode === 'system' ? (dark ? 'dark' : 'light') : mode
}

interface Started {
  handle: EditorHandle
  gpuError: string | null
  attempts: { backend: string; error?: string }[]
  /** The initial `document` could not be opened. */
  loadError: string | null
}

function Shell(props: {
  started: Started
  theme: 'light' | 'dark'
  setThemeMode: (m: ThemeMode) => void
  themeMode: ThemeMode
  name: string
  setName: (n: string) => void
  missing: string[]
  setMissing: (m: string[]) => void
  recovery: StoredMeta | null
  setRecovery: (m: StoredMeta | null) => void
  root: RefObject<HTMLDivElement | null>
  children?: ReactNode
}): ReactNode {
  const h = useEditor()
  const t = useT()
  const doc = useDocument()
  const unit: LengthUnit = doc?.settings?.displayUnit ?? 'millimetre'
  const tool = useEditorState((s) => s.tool)
  const canUndo = useEditorState((s) => s.canUndo)
  const canRedo = useEditorState((s) => s.canRedo)
  const [palette, setPalette] = useState(false)
  const [uiError, setUiError] = useState<UiError | null>(() =>
    props.started.loadError ? { title: t('error.load'), message: props.started.loadError } : null,
  )
  const nameRef = useRef(props.name)
  nameRef.current = props.name
  useEffect(() => {
    h.core.setUnitScale(LENGTH_UNITS[unit].mm)
  }, [h.core, unit])
  const hooks: FileHooks = useMemo(
    () => ({
      name: () => nameRef.current,
      setName: (n) => {
        props.setName(n)
        h.autosave?.setName(n)
      },
      error: (title, e) => setUiError({ title, message: e instanceof Error ? e.message : String(e) }),
      missing: (m) => props.setMissing(m),
      view: () => ({ camera: h.viewport.camera }),
      restoreView: (v) => {
        const cam = (v as { camera?: Camera } | null)?.camera
        if (cam && isValidCamera(cam)) h.viewport.setCamera(cam)
        else void h.viewport.fit()
      },
    }),
    [h, props.setName, props.setMissing],
  )
  // biome-ignore lint/correctness/useExhaustiveDependencies: tools/plugins change rarely; rebuilt when the palette opens
  const actions: Action[] = useMemo(() => buildActions(h, t, hooks), [h, t, hooks, palette])
  const run = useCallback((id: string) => void actions.find((a) => a.id === id)?.run(), [actions])
  const empty = doc !== null && doc.entities.length === 0
  // Editor-wide shortcuts (palette, save, open) work anywhere inside the editor,
  // including panels; tool shortcuts stay on the canvas surface (see bindDom).
  const runRef = useRef(run)
  runRef.current = run
  useEffect(() => {
    const el = props.root.current
    if (!el) return
    const listener = (e: KeyboardEvent): void => {
      const mod = e.ctrlKey || e.metaKey
      const k = e.key.toLowerCase()
      if (mod && k === 'k') {
        e.preventDefault()
        setPalette(true)
      } else if (mod && k === 's') {
        e.preventDefault()
        runRef.current('file.save')
      } else if (mod && k === 'o') {
        e.preventDefault()
        runRef.current('file.open')
      }
    }
    el.addEventListener('keydown', listener)
    return () => el.removeEventListener('keydown', listener)
  }, [props.root])
  return (
    <>
      <header className="dl-topbar">
        <fieldset className="dl-row dl-plain-fieldset">
          <legend className="dl-visually-hidden">{t('menu.file')}</legend>
          <button type="button" className="dl-btn" onClick={() => run('file.new')}>
            {t('menu.new')}
          </button>
          <button type="button" className="dl-btn" onClick={() => run('file.open')}>
            {t('menu.open')}
          </button>
          <button type="button" className="dl-btn" onClick={() => run('file.save')}>
            {t('menu.save')}
          </button>
          <button type="button" className="dl-btn" onClick={() => run('file.exportSvg')}>
            {t('menu.exportSvg')}
          </button>
          <button type="button" className="dl-btn" onClick={() => run('file.exportDxf')}>
            {t('menu.exportDxf')}
          </button>
          {h.canvas && (
            <button type="button" className="dl-btn" onClick={() => run('file.exportPng')}>
              {t('menu.exportPng')}
            </button>
          )}
        </fieldset>
        <span className="dl-muted" data-testid="document-name">
          {props.name}
        </span>
        <span className="dl-spacer" />
        <button
          type="button"
          className="dl-btn dl-icon-btn"
          aria-label={t('menu.undo')}
          title={`${t('menu.undo')} (Ctrl+Z)`}
          disabled={!canUndo}
          onClick={() => void h.core.undo()}
        >
          <Icon name="undo" />
        </button>
        <button
          type="button"
          className="dl-btn dl-icon-btn"
          aria-label={t('menu.redo')}
          title={`${t('menu.redo')} (Ctrl+Shift+Z)`}
          disabled={!canRedo}
          onClick={() => void h.core.redo()}
        >
          <Icon name="redo" />
        </button>
        <button type="button" className="dl-btn" onClick={() => setPalette(true)} title="Ctrl+K">
          <Icon name="palette" /> {t('menu.commands')}
        </button>
        <select
          className="dl-select"
          style={{ width: 'auto' }}
          aria-label={t('menu.theme')}
          value={props.themeMode}
          onChange={(e) => props.setThemeMode(e.target.value as ThemeMode)}
        >
          <option value="light">{t('theme.light')}</option>
          <option value="dark">{t('theme.dark')}</option>
          <option value="system">{t('theme.system')}</option>
        </select>
      </header>
      <Toolbar />
      <div className="dl-canvas-overlay">
        <ErrorBanner extra={uiError} onDismissExtra={() => setUiError(null)} />
        <MissingPluginsBanner types={props.missing} onDismiss={() => props.setMissing([])} />
        {props.started.gpuError && (
          <div className="dl-canvas-message dl-error" role="alert">
            <strong>{t('error.gpu.title')}</strong>
            <p>{t('error.gpu.body')}</p>
            <details>
              <summary>{t('error.details')}</summary>
              <ul>
                {props.started.attempts.map((a) => (
                  <li key={a.backend}>
                    {a.backend}: {a.error}
                  </li>
                ))}
              </ul>
            </details>
          </div>
        )}
        {empty && tool === 'select' && !props.started.gpuError && (
          <div className="dl-canvas-message" role="note">
            <strong>{t('empty.title')}</strong>
            <p className="dl-muted">{t('empty.body')}</p>
          </div>
        )}
      </div>
      <aside className="dl-sidebar" aria-label={t('panel.inspector')}>
        <Inspector />
        <ConstraintsPanel />
        <LayersPanel />
        <ObjectsPanel />
        <PluginPanels />
        {props.children}
      </aside>
      <StatusBar unit={unit} />
      {palette && <CommandPalette actions={actions} onClose={() => setPalette(false)} />}
      {props.recovery && (
        <RecoveryDialog
          meta={props.recovery}
          onRestore={() => {
            const a = h.autosave
            props.setRecovery(null)
            void a?.restore().then(
              (v) => hooks.restoreView(v),
              (e) => setUiError({ title: t('error.load'), message: String(e) }),
            )
          }}
          onDiscard={() => {
            props.setRecovery(null)
            void h.autosave?.discard()
          }}
        />
      )}
    </>
  )
}

/** The complete editor. Give it a sized container (it fills 100% height). */
export function DotloomEditor(props: DotloomEditorProps): ReactNode {
  const [themeMode, setThemeMode] = useState<ThemeMode>(props.theme ?? 'system')
  const theme = useResolvedTheme(themeMode)
  const host = useRef<HTMLDivElement>(null)
  const rootRef = useRef<HTMLDivElement>(null)
  const [started, setStarted] = useState<Started | null>(null)
  const [fatal, setFatal] = useState<string | null>(null)
  const [name, setName] = useState(props.name ?? 'untitled.dotl')
  const [missing, setMissing] = useState<string[]>([])
  const [recovery, setRecovery] = useState<StoredMeta | null>(null)
  const initial = useRef({ props, theme })
  useEffect(() => {
    const p = initial.current.props
    const el = host.current
    if (!el) return
    let cancelled = false
    const cleanup: (() => void)[] = []
    // Register a disposer; if the effect was already cleaned up (StrictMode,
    // fast unmount) dispose right away so nothing created late leaks.
    const own = (dispose: () => void): boolean => {
      if (cancelled) {
        dispose()
        return false
      }
      cleanup.push(dispose)
      return true
    }
    void (async () => {
      const engine = await DotloomEngine.create(p.engine ?? {})
      if (!own(() => engine.dispose())) return
      let canvas: Viewport | null = null
      let gpuError: string | null = null
      let attempts: Started['attempts'] = []
      try {
        const v = await Viewport.create(el, engine, { ...p.viewport, theme: { preset: initial.current.theme } })
        if (!own(() => v.dispose())) return
        canvas = v
      } catch (e) {
        gpuError = e instanceof Error ? e.message : String(e)
        attempts = ((e as { details?: { attempts?: Started['attempts'] } }).details?.attempts ??
          []) as Started['attempts']
      }
      if (cancelled) return
      const viewport = canvas ?? new NullViewport()
      const core = new EditorCore(engine, viewport, p.core)
      for (const tool of builtinTools()) core.registerTool(tool)
      core.start()
      own(() => core.dispose())
      if (canvas) own(bindDom(core, canvas, { label: 'Drawing canvas' }))
      const plugins = new PluginHost(engine, core)
      own(() => void plugins.dispose())
      for (const pl of p.plugins ?? []) {
        try {
          await plugins.register(pl)
        } catch (e) {
          core.report(e)
        }
      }
      let autosave: Autosave | null = null
      if (p.autosave !== false) {
        const opt = typeof p.autosave === 'object' ? p.autosave : {}
        const storage =
          opt.storage ??
          plugins.storageAdapters()[0] ??
          (typeof indexedDB === 'undefined' ? new MemoryStorage() : new IndexedDbStorage())
        const a = new Autosave(engine, storage, {
          ...(opt.key ? { key: opt.key } : {}),
          name: p.name ?? 'untitled.dotl',
          view: () => ({ camera: viewport.camera }),
        })
        own(() => a.dispose())
        autosave = a
      }
      let loadError: string | null = null
      if (p.document) {
        try {
          const r = await engine.load(p.document)
          setMissing(r.missingPlugins)
          await viewport.fit()
        } catch (e) {
          loadError = e instanceof Error ? e.message : String(e)
        }
      } else if (autosave) {
        const rec = await autosave.recoverable().catch(() => null)
        if (rec && !cancelled) setRecovery(rec)
      }
      if (cancelled) return
      autosave?.start()
      const handle: EditorHandle = { engine, core, viewport, canvas, plugins, autosave }
      setStarted({ handle, gpuError, attempts, loadError })
      p.onReady?.(handle)
    })().catch((e) => {
      if (!cancelled) setFatal(e instanceof Error ? e.message : String(e))
    })
    return () => {
      cancelled = true
      for (const f of cleanup.splice(0).reverse()) f()
    }
  }, [])
  useEffect(() => {
    started?.handle.canvas?.setTheme({ preset: theme })
  }, [started, theme])
  const cls = ['dl-editor', props.className].filter(Boolean).join(' ')
  return (
    <I18nProvider
      {...(props.locale ? { locale: props.locale } : {})}
      {...(props.messages ? { messages: props.messages } : {})}
    >
      <div className={cls} style={props.style} data-theme={theme} ref={rootRef}>
        {/* Stable canvas cell: created before the engine starts, never remounted. */}
        <div className="dl-canvas">
          <div ref={host} className="dl-canvas-host" />
        </div>
        {fatal && (
          <div className="dl-canvas-overlay">
            <div className="dl-banner dl-error" role="alert" style={{ margin: 16 }}>
              {fatal}
            </div>
          </div>
        )}
        {started && (
          <EditorProvider value={started.handle}>
            <Shell
              started={started}
              theme={theme}
              themeMode={themeMode}
              setThemeMode={setThemeMode}
              name={name}
              setName={setName}
              missing={missing}
              setMissing={setMissing}
              recovery={recovery}
              setRecovery={setRecovery}
              root={rootRef}
            >
              {props.children}
            </Shell>
          </EditorProvider>
        )}
      </div>
    </I18nProvider>
  )
}

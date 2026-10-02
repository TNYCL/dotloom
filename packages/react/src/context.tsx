/**
 * React bindings: one context with the editor handle and hooks built on
 * `useSyncExternalStore`. Components subscribe to exactly what they render:
 * pointer movement only re-renders the status bar, drag previews never refetch
 * the document (they do not change the revision).
 */

import type {
  Autosave,
  AutosaveState,
  DiagnosticReport,
  DocumentJson,
  DotloomEngine,
  EditorCore,
  EditorState,
  EntityId,
  EntityInfo,
  EntityTypeDef,
  PluginHost,
  PointerState,
  SolveStatus,
  Viewport,
  ViewportLike,
} from '@dotloomjs/sdk'
import { createContext, type ReactNode, useContext, useEffect, useState, useSyncExternalStore } from 'react'

export interface EditorHandle {
  engine: DotloomEngine
  core: EditorCore
  viewport: ViewportLike
  /** The GPU viewport, or `null` when rendering is unavailable. */
  canvas: Viewport | null
  plugins: PluginHost
  autosave: Autosave | null
}

const Ctx = createContext<EditorHandle | null>(null)

export function EditorProvider(props: { value: EditorHandle; children: ReactNode }): ReactNode {
  return <Ctx.Provider value={props.value}>{props.children}</Ctx.Provider>
}

/** The editor handle (throws outside `EditorProvider`). */
export function useEditor(): EditorHandle {
  const h = useContext(Ctx)
  if (!h) throw new Error('useEditor() must be used inside <EditorProvider> or <DotloomEditor>')
  return h
}

/** Select a slice of the editor state; re-renders only when it changes. */
export function useEditorState<T>(select: (s: EditorState) => T): T {
  const { core } = useEditor()
  return useSyncExternalStore(
    core.state.subscribe,
    () => select(core.state.getSnapshot()),
    () => select(core.state.getSnapshot()),
  )
}

export function usePointer(): PointerState {
  const { core } = useEditor()
  return useSyncExternalStore(core.pointer.subscribe, core.pointer.getSnapshot, core.pointer.getSnapshot)
}

export function useAutosaveState(): AutosaveState | null {
  const { autosave } = useEditor()
  const sub = autosave ? autosave.state.subscribe : () => () => undefined
  const get = autosave ? autosave.state.getSnapshot : () => null
  return useSyncExternalStore(sub, get, get)
}

/** Fetch something whenever the committed revision changes (latest result wins). */
export function useRevisionQuery<T>(fetch: (engine: DotloomEngine) => Promise<T>, deps: unknown[] = []): T | null {
  const { engine } = useEditor()
  const revision = useEditorState((s) => s.revision)
  const [value, setValue] = useState<T | null>(null)
  // biome-ignore lint/correctness/useExhaustiveDependencies: revision and caller deps drive refetching
  useEffect(() => {
    let live = true
    fetch(engine).then(
      (v) => {
        if (live) setValue(v)
      },
      () => {
        if (live) setValue(null)
      },
    )
    return () => {
      live = false
    }
  }, [engine, revision, ...deps])
  return value
}

/** The document (refetched per commit; previews do not trigger it). */
export function useDocument(): DocumentJson | null {
  return useRevisionQuery((e) => e.documentJson())
}

export function useEntityInfo(id: EntityId | null): EntityInfo | null {
  return useRevisionQuery((e) => (id === null ? Promise.resolve(null) : e.entityInfo(id)), [id])
}

export function useAnalysis(): { status: SolveStatus; diagnostics: DiagnosticReport[] } | null {
  return useRevisionQuery((e) => e.analyze())
}

/** Registered plugin type definitions (for labels and dimensions of properties). */
export function usePluginTypes(): Map<string, EntityTypeDef> {
  const { plugins, engine } = useEditor()
  const [types, setTypes] = useState(new Map<string, EntityTypeDef>())
  useEffect(() => {
    let live = true
    const load = (): void => {
      engine.pluginTypes().then(
        (list) => {
          if (live) setTypes(new Map(list.map((t) => [t.typeId, t.definition])))
        },
        () => undefined,
      )
    }
    load()
    const off = plugins.subscribe(load)
    return () => {
      live = false
      off()
    }
  }, [engine, plugins])
  return types
}

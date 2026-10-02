import type { StoredMeta } from '@dotloom/sdk'
import { type ReactNode, useEffect, useRef } from 'react'
import { useEditor, useEditorState } from '../context.js'
import { useT } from '../i18n.js'

/** Modal with focus moved inside and restored on close; Escape cancels. */
export function Modal(props: { label: string; onCancel: () => void; children: ReactNode }): ReactNode {
  const box = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const prev = document.activeElement as HTMLElement | null
    box.current?.querySelector<HTMLElement>('button, input, select, [tabindex]')?.focus()
    return () => prev?.focus?.()
  }, [])
  return (
    <div className="dl-dialog-backdrop">
      <div
        ref={box}
        className="dl-dialog"
        role="dialog"
        aria-modal="true"
        aria-label={props.label}
        onKeyDown={(e) => {
          if (e.key === 'Escape') {
            e.stopPropagation()
            props.onCancel()
          }
        }}
      >
        {props.children}
      </div>
    </div>
  )
}

export function RecoveryDialog(props: { meta: StoredMeta; onRestore: () => void; onDiscard: () => void }): ReactNode {
  const t = useT()
  return (
    <Modal label={t('recovery.title')} onCancel={props.onDiscard}>
      <h2 style={{ marginTop: 0, fontSize: 16 }}>{t('recovery.title')}</h2>
      <p>{t('recovery.body', { name: props.meta.name, time: new Date(props.meta.savedAt).toLocaleString() })}</p>
      <div className="dl-row" style={{ justifyContent: 'flex-end' }}>
        <button type="button" className="dl-btn" onClick={props.onDiscard}>
          {t('recovery.discard')}
        </button>
        <button type="button" className="dl-btn dl-primary" onClick={props.onRestore}>
          {t('recovery.restore')}
        </button>
      </div>
    </Modal>
  )
}

export interface UiError {
  title: string
  message: string
  details?: string[]
}

/** Error of the last failed command (from the editor state) or a UI error. */
export function ErrorBanner(props: { extra: UiError | null; onDismissExtra: () => void }): ReactNode {
  const { core } = useEditor()
  const t = useT()
  const err = useEditorState((s) => s.error)
  const shown: UiError | null =
    props.extra ??
    (err
      ? {
          title: t('error.title'),
          message: err.message,
          details: err.diagnostics.flatMap((d) => d.labels),
        }
      : null)
  if (!shown) return null
  return (
    <div className="dl-banner dl-error" role="alert" style={{ margin: 8 }}>
      <strong>{shown.title}</strong> — {shown.message}
      {shown.details && shown.details.length > 0 && (
        <div>
          {t('error.conflict')} {shown.details.join('; ')}
        </div>
      )}
      <div>
        <button
          type="button"
          className="dl-btn"
          onClick={() => (props.extra ? props.onDismissExtra() : core.clearError())}
        >
          {t('error.dismiss')}
        </button>
      </div>
    </div>
  )
}

export function MissingPluginsBanner(props: { types: string[]; onDismiss: () => void }): ReactNode {
  const t = useT()
  if (props.types.length === 0) return null
  return (
    <div className="dl-banner" role="status" style={{ margin: 8 }}>
      <strong>{t('missing.title')}</strong> — {t('missing.body', { types: props.types.join(', ') })}{' '}
      <button type="button" className="dl-btn" onClick={props.onDismiss}>
        {t('error.dismiss')}
      </button>
    </div>
  )
}

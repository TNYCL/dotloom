import { type ReactNode, useEffect, useMemo, useRef, useState } from 'react'
import type { Action } from '../actions.js'
import { useT } from '../i18n.js'

/** Score a query against a label: substring, then word initials. */
export function matchScore(label: string, query: string): number {
  const l = label.toLocaleLowerCase()
  const q = query.trim().toLocaleLowerCase()
  if (!q) return 1
  const i = l.indexOf(q)
  if (i >= 0) return 100 - i
  const initials = l
    .split(/[\s:/()-]+/)
    .filter(Boolean)
    .map((w) => w[0])
    .join('')
  return initials.includes(q) ? 10 : 0
}

/** Modal command search (Mod+K). Arrow keys move, Enter runs, Escape closes. */
export function CommandPalette(props: { actions: Action[]; onClose: () => void }): ReactNode {
  const t = useT()
  const [q, setQ] = useState('')
  const [index, setIndex] = useState(0)
  const input = useRef<HTMLInputElement>(null)
  const results = useMemo(
    () =>
      props.actions
        .map((a) => ({ a, s: matchScore(a.label, q) }))
        .filter((x) => x.s > 0)
        .sort((x, y) => y.s - x.s)
        .map((x) => x.a),
    [props.actions, q],
  )
  useEffect(() => {
    const prev = document.activeElement as HTMLElement | null
    input.current?.focus()
    return () => prev?.focus?.()
  }, [])
  const run = (a: Action | undefined): void => {
    if (!a) return
    props.onClose()
    void a.run()
  }
  return (
    <div className="dl-dialog-backdrop">
      <div className="dl-dialog" role="dialog" aria-modal="true" aria-label={t('menu.commands')}>
        <input
          ref={input}
          className="dl-input"
          role="combobox"
          aria-expanded="true"
          aria-controls="dl-palette-list"
          aria-activedescendant={results[index] ? `dl-cmd-${results[index].id}` : undefined}
          placeholder={t('palette.placeholder')}
          value={q}
          onChange={(e) => {
            setQ(e.target.value)
            setIndex(0)
          }}
          onKeyDown={(e) => {
            if (e.key === 'ArrowDown') {
              e.preventDefault()
              setIndex((i) => Math.min(results.length - 1, i + 1))
            } else if (e.key === 'ArrowUp') {
              e.preventDefault()
              setIndex((i) => Math.max(0, i - 1))
            } else if (e.key === 'Enter') {
              e.preventDefault()
              run(results[index])
            } else if (e.key === 'Escape') {
              e.preventDefault()
              props.onClose()
            } else if (e.key === 'Tab') {
              e.preventDefault()
            }
          }}
        />
        {results.length === 0 ? (
          <p className="dl-muted">{t('palette.empty')}</p>
        ) : (
          <div id="dl-palette-list" className="dl-palette-list" role="listbox" aria-label={t('menu.commands')}>
            {results.map((a, i) => (
              <div
                key={a.id}
                id={`dl-cmd-${a.id}`}
                role="option"
                tabIndex={-1}
                aria-selected={i === index}
                onMouseEnter={() => setIndex(i)}
                onClick={() => run(a)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') run(a)
                }}
              >
                <span>{a.label}</span>
                {a.shortcut && <span className="dl-kbd">{a.shortcut}</span>}
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}

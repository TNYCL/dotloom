import type { Constraint, DocumentJson, Entity, EntityId, LineRef, RuleSpec } from '@dotloomjs/sdk'
import { type ReactNode, useMemo } from 'react'
import { useAnalysis, useDocument, useEditor, useEditorState } from '../context.js'
import { type Translate, useT } from '../i18n.js'
import { Icon } from './icons.js'

/** Entity IDs a rule refers to. */
export function ruleEntities(r: RuleSpec): EntityId[] {
  const out = new Set<EntityId>()
  const walk = (v: unknown): void => {
    if (!v || typeof v !== 'object') return
    if (Array.isArray(v)) {
      for (const x of v) walk(x)
      return
    }
    for (const [k, x] of Object.entries(v)) {
      if ((k === 'entity' || k === 'circle') && typeof x === 'number') out.add(x)
      else if (
        (k === 'a' || k === 'b') &&
        typeof x === 'number' &&
        (r.kind === 'concentric' || r.kind === 'equalRadius' || r.kind === 'tangentCircles')
      )
        out.add(x)
      else walk(x)
    }
  }
  walk(r)
  return [...out]
}

function describeRule(c: Constraint, t: Translate): string {
  if (c.label) return c.label
  const known: Record<string, string> = {
    horizontal: t('rules.horizontal'),
    vertical: t('rules.vertical'),
    length: t('rules.length'),
    parallel: t('rules.parallel'),
    perpendicular: t('rules.perpendicular'),
    equalLength: t('rules.equalLength'),
    concentric: t('rules.concentric'),
    equalRadius: t('rules.equalRadius'),
    coincident: t('rules.coincident'),
  }
  return known[c.rule.kind] ?? c.rule.kind
}

const lineOf = (id: EntityId): LineRef => ({ from: { entity: id, anchor: 'start' }, to: { entity: id, anchor: 'end' } })

function geometryKind(e: Entity | undefined): string {
  return e?.geometry?.type ?? 'other'
}

/** Rule templates for the current selection of built-in shapes. */
function templates(sel: Entity[], t: Translate): { id: string; label: string; rule: () => RuleSpec }[] {
  const kinds = sel.map(geometryKind)
  const [a, b] = sel
  const out: { id: string; label: string; rule: () => RuleSpec }[] = []
  if (sel.length === 1 && a && kinds[0] === 'line') {
    out.push(
      {
        id: 'horizontal',
        label: t('rules.horizontal'),
        rule: () => ({ kind: 'horizontal', a: { entity: a.id, anchor: 'start' }, b: { entity: a.id, anchor: 'end' } }),
      },
      {
        id: 'vertical',
        label: t('rules.vertical'),
        rule: () => ({ kind: 'vertical', a: { entity: a.id, anchor: 'start' }, b: { entity: a.id, anchor: 'end' } }),
      },
      {
        id: 'length',
        label: t('rules.length'),
        rule: () => {
          const g = a.geometry as { a: [number, number]; b: [number, number] }
          return { kind: 'length', line: lineOf(a.id), value: Math.hypot(g.b[0] - g.a[0], g.b[1] - g.a[1]) }
        },
      },
    )
  }
  if (sel.length === 2 && a && b && kinds.every((k) => k === 'line')) {
    out.push(
      {
        id: 'parallel',
        label: t('rules.parallel'),
        rule: () => ({ kind: 'parallel', a: lineOf(a.id), b: lineOf(b.id) }),
      },
      {
        id: 'perpendicular',
        label: t('rules.perpendicular'),
        rule: () => ({ kind: 'perpendicular', a: lineOf(a.id), b: lineOf(b.id) }),
      },
      {
        id: 'equalLength',
        label: t('rules.equalLength'),
        rule: () => ({ kind: 'equalLength', a: lineOf(a.id), b: lineOf(b.id) }),
      },
      {
        id: 'coincident',
        label: t('rules.coincident'),
        rule: () => ({ kind: 'coincident', a: { entity: a.id, anchor: 'end' }, b: { entity: b.id, anchor: 'start' } }),
      },
    )
  }
  if (sel.length === 2 && a && b && kinds.every((k) => k === 'circle' || k === 'arc')) {
    out.push(
      { id: 'concentric', label: t('rules.concentric'), rule: () => ({ kind: 'concentric', a: a.id, b: b.id }) },
      { id: 'equalRadius', label: t('rules.equalRadius'), rule: () => ({ kind: 'equalRadius', a: a.id, b: b.id }) },
    )
  }
  return out
}

/** Rules of the selection: status, enable/disable, remove, add from templates. */
export function ConstraintsPanel(): ReactNode {
  const { core, plugins } = useEditor()
  const t = useT()
  const doc: DocumentJson | null = useDocument()
  const analysis = useAnalysis()
  const selection = useEditorState((s) => s.selection)
  const sel = useMemo(() => new Set(selection), [selection])
  const rules = (doc?.constraints ?? []).filter(
    (c) => ruleEntities(c.rule).some((id) => sel.has(id)) || (c.owner !== undefined && sel.has(c.owner)),
  )
  const selEntities = (doc?.entities ?? []).filter((e) => sel.has(e.id))
  const tpl = templates(selEntities, t)
  const pluginTpl = plugins.constraintTemplates().filter((x) => x.template.arity === selection.length)
  const status = analysis?.status
  const statusText =
    status === undefined
      ? '…'
      : status.status === 'underconstrained'
        ? t('rules.status.underconstrained', { dof: status.dof })
        : t(`rules.status.${status.status}`)
  return (
    <section className="dl-panel" aria-labelledby="dl-rules-h">
      <h2 id="dl-rules-h">{t('panel.constraints')}</h2>
      <p
        role="status"
        className={
          status?.status === 'conflicting' || status?.status === 'notConverged' ? 'dl-banner dl-error' : 'dl-muted'
        }
      >
        {statusText}
      </p>
      {analysis?.diagnostics
        .filter((d) => d.kind === 'conflict' || d.kind === 'suspectedConflict' || d.kind === 'notConverged')
        .map((d) => (
          <div key={`${d.kind}-${d.constraints.join(',')}`} className="dl-banner dl-error">
            {d.message}
            {d.labels.length > 0 && <div className="dl-muted">{d.labels.join('; ')}</div>}
          </div>
        ))}
      {selection.length > 0 && rules.length === 0 && <p className="dl-muted">{t('rules.none')}</p>}
      <ul style={{ listStyle: 'none', margin: 0, padding: 0 }}>
        {rules.map((c) => (
          <li key={c.id} className="dl-row">
            <input
              type="checkbox"
              aria-label={`${t('rules.enabled')}: ${describeRule(c, t)}`}
              checked={c.enabled !== false}
              onChange={(e) =>
                void core.apply({
                  label: 'Toggle rule',
                  commands: [{ op: 'updateConstraint', id: c.id, patch: { enabled: e.target.checked } }],
                })
              }
            />
            <span style={{ flex: 1 }}>
              {describeRule(c, t)}
              {c.strength && c.strength !== 'required' && <span className="dl-badge"> {c.strength}</span>}
            </span>
            <button
              type="button"
              className="dl-btn dl-icon-btn"
              aria-label={`${t('rules.remove')}: ${describeRule(c, t)}`}
              onClick={() =>
                void core.apply({ label: 'Remove rule', commands: [{ op: 'removeConstraint', id: c.id }] })
              }
            >
              <Icon name="trash" size={16} />
            </button>
          </li>
        ))}
      </ul>
      {(tpl.length > 0 || pluginTpl.length > 0) && (
        <fieldset className="dl-fieldset">
          <legend className="dl-visually-hidden">{t('rules.add')}</legend>
          {tpl.map((x) => (
            <button
              key={x.id}
              type="button"
              className="dl-btn"
              onClick={() =>
                void core.apply({ label: x.label, commands: [{ op: 'addConstraint', constraint: { rule: x.rule() } }] })
              }
            >
              + {x.label}
            </button>
          ))}
          {pluginTpl.map(({ plugin, template }) => (
            <button
              key={`${plugin}:${template.id}`}
              type="button"
              className="dl-btn"
              onClick={() =>
                void core.apply({
                  label: template.label,
                  commands: template
                    .build(selection)
                    .map((constraint) => ({ op: 'addConstraint' as const, constraint })),
                })
              }
            >
              + {template.label}
            </button>
          ))}
        </fieldset>
      )}
    </section>
  )
}

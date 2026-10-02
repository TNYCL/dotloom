import {
  type CommitReport,
  type Constraint,
  type DimName,
  DotloomError,
  type EntityId,
  type EntityInfo,
  type EntityTypeDef,
  formatAngle,
  formatDuration,
  formatLength,
  LENGTH_UNITS,
  type LengthUnit,
  type ParamRef,
  type PropDef,
  type PropValue,
  parseAngle,
  parseDuration,
  parseLength,
  type Transaction,
} from '@dotloom/sdk'
import { type ReactNode, useEffect, useId, useState } from 'react'
import { useDocument, useEditor, useEditorState, useEntityInfo, usePluginTypes } from '../context.js'
import { type Translate, useT } from '../i18n.js'
import { Icon } from './icons.js'

const ANGLE_PARAMS = new Set(['start', 'sweep', 'rotation'])

/** Dimension of a parameter: from the plugin definition, or by name for built-ins. */
function dimOf(name: string, def: EntityTypeDef | undefined): DimName {
  const p = def?.props?.[name.split('.')[0] ?? name]
  if (p && p.type === 'number') return p.dim ?? 'scalar'
  if (p && p.type === 'point') return 'length'
  return ANGLE_PARAMS.has(name) ? 'angle' : 'length'
}

export function formatValue(v: number, dim: DimName, unit: LengthUnit): string {
  if (dim === 'length') return formatLength(v, unit, undefined, false)
  if (dim === 'angle') return formatAngle(v).replace('°', '')
  if (dim === 'time') return formatDuration(v)
  return String(Math.round(v * 1e6) / 1e6)
}

export function parseValue(text: string, dim: DimName, unit: LengthUnit): number | null {
  if (dim === 'length') return parseLength(text, unit)
  if (dim === 'angle') return parseAngle(text)
  if (dim === 'time') return parseDuration(text)
  const v = Number(text.trim().replace(',', '.'))
  return Number.isFinite(v) ? v : null
}

function unitLabel(dim: DimName, unit: LengthUnit): string {
  return dim === 'length' ? LENGTH_UNITS[unit].symbol : dim === 'angle' ? '°' : ''
}

function paramRef(entity: EntityId, name: string, builtin: boolean): ParamRef {
  return builtin ? { entity, geom: name } : { entity, prop: name }
}

function sameRef(a: ParamRef, b: ParamRef): boolean {
  return a.entity === b.entity && JSON.stringify(a) === JSON.stringify(b)
}

interface FieldError {
  message: string
  nearest?: number
}

function describe(e: unknown, t: Translate, entity: EntityId, param: string): FieldError {
  if (e instanceof DotloomError) {
    const d = e.details as
      | {
          failure?: {
            nearest?: { entity: number; param: string; feasible: number }[]
            diagnostics?: { labels: string[] }[]
          }
        }
      | undefined
    const near = d?.failure?.nearest?.find((n) => n.entity === entity && n.param === param)
    const labels = d?.failure?.diagnostics?.flatMap((x) => x.labels) ?? []
    const msg = labels.length > 0 ? `${t('error.conflict')} ${labels.join('; ')}` : e.message
    return near ? { message: msg, nearest: near.feasible } : { message: msg }
  }
  return { message: e instanceof Error ? e.message : String(e) }
}

/** A numeric parameter with units, lock toggle and inline solver feedback. */
function NumberField(props: {
  entity: EntityId
  name: string
  label: string
  value: number
  dim: DimName
  unit: LengthUnit
  readOnly: boolean
  locked: Constraint | null
  builtin: boolean
}): ReactNode {
  const { core } = useEditor()
  const t = useT()
  const id = useId()
  const shown = formatValue(props.value, props.dim, props.unit)
  const [text, setText] = useState(shown)
  const [error, setError] = useState<FieldError | null>(null)
  const [busy, setBusy] = useState(false)
  useEffect(() => setText(shown), [shown])
  const commit = async (value: number): Promise<void> => {
    setBusy(true)
    try {
      await core.applyOrThrow({
        label: `Set ${props.label}`,
        commands: [{ op: 'setParams', values: [{ entity: props.entity, param: props.name, value }], mode: 'exact' }],
      })
      setError(null)
    } catch (e) {
      setError(describe(e, t, props.entity, props.name))
    } finally {
      setBusy(false)
    }
  }
  const submit = (): void => {
    if (text.trim() === shown) {
      setError(null)
      return
    }
    const v = parseValue(text, props.dim, props.unit)
    if (v === null) {
      setError({ message: t('inspector.invalid') })
      return
    }
    void commit(v)
  }
  const toggleLock = (): void => {
    const tx: Transaction = props.locked
      ? { label: 'Unlock', commands: [{ op: 'removeConstraint', id: props.locked.id }] }
      : {
          label: 'Lock',
          commands: [
            {
              op: 'addConstraint',
              constraint: {
                rule: { kind: 'fix', param: paramRef(props.entity, props.name, props.builtin), value: props.value },
                label: `${props.label} locked`,
              },
            },
          ],
        }
    void core.apply(tx)
  }
  const sym = unitLabel(props.dim, props.unit)
  return (
    <div className="dl-field">
      <label htmlFor={id} title={props.name}>
        {props.label}
      </label>
      <div className="dl-row">
        <input
          id={id}
          className="dl-input"
          inputMode="decimal"
          value={text}
          readOnly={props.readOnly}
          aria-invalid={error !== null}
          aria-describedby={error ? `${id}-err` : undefined}
          aria-busy={busy}
          onChange={(e) => setText(e.target.value)}
          onBlur={submit}
          onKeyDown={(e) => {
            if (e.key === 'Enter') submit()
            if (e.key === 'Escape') {
              setText(shown)
              setError(null)
            }
          }}
        />
        <span className="dl-muted" style={{ minWidth: 18 }}>
          {sym}
        </span>
      </div>
      <button
        type="button"
        className="dl-btn dl-icon-btn"
        aria-pressed={props.locked !== null}
        aria-label={`${props.locked ? t('inspector.unlock') : t('inspector.lock')}: ${props.label}`}
        title={props.locked ? t('inspector.unlock') : t('inspector.lock')}
        disabled={props.readOnly}
        onClick={toggleLock}
      >
        <Icon name={props.locked ? 'lock' : 'unlock'} size={16} />
      </button>
      {error && (
        <div className="dl-field-error" id={`${id}-err`} role="alert">
          {error.message}
          {error.nearest !== undefined && (
            <div>
              <button
                type="button"
                className="dl-btn"
                onClick={() => {
                  const n = error.nearest as number
                  setText(formatValue(n, props.dim, props.unit))
                  void commit(n)
                }}
              >
                {t('inspector.nearest', {
                  value: `${formatValue(error.nearest, props.dim, props.unit)} ${sym}`.trim(),
                })}
              </button>
            </div>
          )}
        </div>
      )}
    </div>
  )
}

function OtherField(props: {
  entity: EntityId
  name: string
  def: PropDef
  value: PropValue | undefined
  readOnly: boolean
}): ReactNode {
  const { core } = useEditor()
  const id = useId()
  const set = (v: PropValue): void => {
    void core.apply({
      label: `Set ${props.name}`,
      commands: [{ op: 'updateEntity', id: props.entity, patch: { props: { [props.name]: v } } }],
    })
  }
  const label = props.def.label ?? props.name
  let control: ReactNode = null
  if (props.def.type === 'enum') {
    control = (
      <select
        id={id}
        className="dl-select"
        disabled={props.readOnly}
        value={typeof props.value === 'string' ? props.value : (props.def.default ?? '')}
        onChange={(e) => set(e.target.value)}
      >
        {props.def.values.map((v) => (
          <option key={v} value={v}>
            {v}
          </option>
        ))}
      </select>
    )
  } else if (props.def.type === 'bool') {
    control = (
      <input
        id={id}
        type="checkbox"
        disabled={props.readOnly}
        checked={props.value === true}
        onChange={(e) => set(e.target.checked)}
      />
    )
  } else if (props.def.type === 'text') {
    control = (
      <input
        id={id}
        className="dl-input"
        disabled={props.readOnly}
        defaultValue={typeof props.value === 'string' ? props.value : ''}
        onBlur={(e) => set(e.currentTarget.value)}
      />
    )
  } else {
    return null
  }
  return (
    <div className="dl-field">
      <label htmlFor={id}>{label}</label>
      {control}
      <span />
    </div>
  )
}

function readOnlyText(info: EntityInfo, t: Translate): string | null {
  const r = info.readOnly
  if (!r) return null
  return t(`inspector.readOnly.${r.reason}`, { type: r.type_id })
}

function EntityInspector(props: { id: EntityId }): ReactNode {
  const { core } = useEditor()
  const t = useT()
  const info = useEntityInfo(props.id)
  const doc = useDocument()
  const types = usePluginTypes()
  if (!info || !doc) return <p className="dl-muted">…</p>
  const e = info.entity
  const unit: LengthUnit = doc.settings?.displayUnit ?? 'millimetre'
  const builtin = e.type.startsWith('dotloom.')
  const def = types.get(e.type)
  const roText = readOnlyText(info, t)
  const readOnly = roText !== null || e.locked === true
  const fixes = doc.constraints.filter((c) => c.rule.kind === 'fix')
  const lockOf = (name: string): Constraint | null =>
    fixes.find((c) => c.rule.kind === 'fix' && sameRef(c.rule.param, paramRef(e.id, name, builtin))) ?? null
  const numeric = def
    ? Object.keys(info.params).filter((n) => {
        const root = n.split('.')[0] ?? n
        return def.props?.[root] !== undefined
      })
    : Object.keys(info.params)
  const others = def ? Object.entries(def.props ?? {}).filter(([, p]) => p.type !== 'number' && p.type !== 'point') : []
  const update = (patch: Record<string, unknown>, label: string): void => {
    void core.apply({ label, commands: [{ op: 'updateEntity', id: e.id, patch }] })
  }
  return (
    <div>
      <div className="dl-row">
        <strong>{def?.label ?? e.type.replace(/^dotloom\./, '')}</strong>
        <span className="dl-muted">#{e.id}</span>
      </div>
      {roText && <div className="dl-banner">{roText}</div>}
      {info.error && <div className="dl-banner dl-error">{info.error}</div>}
      <p className="dl-muted" style={{ margin: '4px 0' }}>
        {t('inspector.unitHint', { unit: LENGTH_UNITS[unit].symbol })}
      </p>
      <div className="dl-field">
        <label htmlFor={`dl-name-${e.id}`}>{t('inspector.name')}</label>
        <input
          id={`dl-name-${e.id}`}
          key={`name-${e.id}-${e.name ?? ''}`}
          className="dl-input"
          defaultValue={e.name ?? ''}
          disabled={roText !== null}
          onBlur={(ev) => {
            if (ev.currentTarget.value !== (e.name ?? '')) update({ name: ev.currentTarget.value }, 'Rename')
          }}
        />
        <span />
      </div>
      <div className="dl-field">
        <label htmlFor={`dl-layer-${e.id}`}>{t('inspector.layer')}</label>
        <select
          id={`dl-layer-${e.id}`}
          className="dl-select"
          value={e.layer}
          disabled={roText !== null}
          onChange={(ev) => update({ layer: Number(ev.target.value) }, 'Change layer')}
        >
          {doc.layers.map((l) => (
            <option key={l.id} value={l.id}>
              {l.name}
            </option>
          ))}
        </select>
        <span />
      </div>
      <div className="dl-row">
        <label>
          <input
            type="checkbox"
            checked={e.locked === true}
            onChange={(ev) => update({ locked: ev.target.checked }, 'Lock')}
          />{' '}
          {t('inspector.locked')}
        </label>
        <label>
          <input
            type="checkbox"
            checked={e.hidden === true}
            onChange={(ev) => update({ hidden: ev.target.checked }, 'Hide')}
          />{' '}
          {t('inspector.hidden')}
        </label>
      </div>
      {numeric.map((n) => {
        const v = info.params[n] as number
        const root = n.split('.')[0] ?? n
        const pd = def?.props?.[root]
        const base = pd && 'label' in pd && pd.label ? pd.label : n
        const label = n.includes('.') && pd ? `${base} ${n.slice(root.length + 1)}` : base
        return (
          <NumberField
            key={`${e.id}-${n}`}
            entity={e.id}
            name={n}
            label={label}
            value={v}
            dim={dimOf(n, def)}
            unit={unit}
            readOnly={readOnly}
            locked={lockOf(n)}
            builtin={builtin}
          />
        )
      })}
      {others.map(([name, pd]) => (
        <OtherField key={name} entity={e.id} name={name} def={pd} value={e.props?.[name]} readOnly={readOnly} />
      ))}
      {info.measured !== null && <p>= {formatLength(info.measured, unit)}</p>}
    </div>
  )
}

function DocumentSettings(): ReactNode {
  const { core, viewport } = useEditor()
  const t = useT()
  const doc = useDocument()
  if (!doc) return null
  const unit: LengthUnit = doc.settings?.displayUnit ?? 'millimetre'
  const set = (patch: { displayUnit?: LengthUnit; gridSpacing?: number; title?: string }): void => {
    void core.apply({ label: 'Settings', commands: [{ op: 'setSettings', patch }] })
  }
  return (
    <div>
      <p className="dl-muted">{t('inspector.none')}</p>
      <h3 style={{ fontSize: 13, margin: '8px 0 4px' }}>{t('panel.document')}</h3>
      <div className="dl-field">
        <label htmlFor="dl-doc-title">{t('document.title')}</label>
        <input
          id="dl-doc-title"
          key={doc.meta?.title ?? ''}
          className="dl-input"
          defaultValue={doc.meta?.title ?? ''}
          onBlur={(e) => {
            if (e.currentTarget.value !== (doc.meta?.title ?? '')) set({ title: e.currentTarget.value })
          }}
        />
        <span />
      </div>
      <div className="dl-field">
        <label htmlFor="dl-doc-unit">{t('document.unit')}</label>
        <select
          id="dl-doc-unit"
          className="dl-select"
          value={unit}
          onChange={(e) => set({ displayUnit: e.target.value as LengthUnit })}
        >
          {(Object.keys(LENGTH_UNITS) as LengthUnit[]).map((u) => (
            <option key={u} value={u}>
              {LENGTH_UNITS[u].symbol}
            </option>
          ))}
        </select>
        <span />
      </div>
      <div className="dl-field">
        <label htmlFor="dl-doc-grid">{t('document.grid')}</label>
        <input
          id="dl-doc-grid"
          key={`${viewport.grid.spacing}-${unit}`}
          className="dl-input"
          defaultValue={formatLength(viewport.grid.spacing, unit, undefined, false)}
          onBlur={(e) => {
            const v = parseLength(e.currentTarget.value, unit)
            if (v !== null && v > 0) {
              viewport.setGrid({ spacing: v })
              set({ gridSpacing: v })
            }
          }}
        />
        <span className="dl-muted">{LENGTH_UNITS[unit].symbol}</span>
      </div>
    </div>
  )
}

function MultiInspector(props: { ids: EntityId[] }): ReactNode {
  const { core } = useEditor()
  const t = useT()
  return (
    <div>
      <p>{t('inspector.many', { n: props.ids.length })}</p>
      <div className="dl-row">
        <button type="button" className="dl-btn" onClick={() => void core.deleteSelection()}>
          <Icon name="trash" size={16} /> {t('inspector.delete')}
        </button>
        <button type="button" className="dl-btn" onClick={() => void core.run('group')}>
          {t('inspector.group')}
        </button>
      </div>
    </div>
  )
}

/** Properties of the selection, or document settings when nothing is selected. */
export function Inspector(): ReactNode {
  const t = useT()
  const selection = useEditorState((s) => s.selection)
  return (
    <section className="dl-panel" aria-labelledby="dl-inspector-h">
      <h2 id="dl-inspector-h">{t('panel.inspector')}</h2>
      {selection.length === 0 && <DocumentSettings />}
      {selection.length === 1 && selection[0] !== undefined && <EntityInspector id={selection[0]} />}
      {selection.length > 1 && <MultiInspector ids={selection} />}
    </section>
  )
}

export type { CommitReport }

/**
 * DOM wiring: translates pointer, wheel, touch and keyboard events of a viewport
 * into `EditorCore` input. Keyboard shortcuts are only read from the focused
 * editor container — never from form fields — so typing in an inspector field
 * can never delete drawing content.
 */

import type { Viewport } from '../viewport.js'
import type { EditorCore } from './core.js'
import type { InputKey, InputPointer } from './types.js'

export interface DomBindingOptions {
  /** Wheel zooms (default) or pans (`'pan'`; Ctrl/⌘+wheel and pinch still zoom). */
  wheel?: 'zoom' | 'pan'
  /** Accessible label of the focusable editor surface. */
  label?: string
}

const isMac = (): boolean =>
  typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent)

/** Whether keyboard input belongs to a text field rather than the editor. */
export function isEditableTarget(t: EventTarget | null): boolean {
  if (!t || typeof (t as HTMLElement).tagName !== 'string') return false
  const el = t as HTMLElement
  const tag = el.tagName.toLowerCase()
  if (tag === 'textarea' || tag === 'select') return true
  if (tag === 'input') {
    const type = (el as HTMLInputElement).type
    return !['button', 'checkbox', 'radio', 'range', 'color', 'file', 'submit', 'reset'].includes(type)
  }
  return el.isContentEditable
}

/** Attach listeners; returns a function that removes them all. */
export function bindDom(core: EditorCore, viewport: Viewport, opts: DomBindingOptions = {}): () => void {
  const host = viewport.container
  const off: (() => void)[] = []
  const on = <K extends keyof HTMLElementEventMap>(
    el: HTMLElement | Window,
    type: K,
    fn: (e: HTMLElementEventMap[K]) => void,
    o?: AddEventListenerOptions,
  ): void => {
    el.addEventListener(type, fn as EventListener, o)
    off.push(() => el.removeEventListener(type, fn as EventListener, o))
  }
  if (!host.hasAttribute('tabindex')) host.tabIndex = 0
  host.setAttribute('role', 'application')
  host.setAttribute('aria-label', opts.label ?? 'Drawing editor')
  host.setAttribute('aria-roledescription', 'drawing canvas')

  const mac = isMac()
  const point = (e: PointerEvent | MouseEvent): InputPointer => {
    const r = viewport.canvas.getBoundingClientRect()
    const pe = e as PointerEvent
    return {
      x: e.clientX - r.left,
      y: e.clientY - r.top,
      button: e.button,
      buttons: e.buttons,
      shift: e.shiftKey,
      mod: mac ? e.metaKey : e.ctrlKey,
      alt: e.altKey,
      pointerId: pe.pointerId ?? 1,
      pointerType: (pe.pointerType as InputPointer['pointerType']) || 'mouse',
    }
  }

  // Panning gestures handled here (not by tools): middle button, Space+drag, two-finger touch.
  let panFrom: { x: number; y: number } | null = null
  let spaceDown = false
  const touches = new Map<number, { x: number; y: number }>()
  let pinch: { d: number; cx: number; cy: number } | null = null

  const pinchState = (): { d: number; cx: number; cy: number } | null => {
    const pts = [...touches.values()]
    const [a, b] = pts
    if (!a || !b) return null
    return { d: Math.hypot(b.x - a.x, b.y - a.y), cx: (a.x + b.x) / 2, cy: (a.y + b.y) / 2 }
  }

  on(host, 'pointerdown', (e) => {
    if (e.target !== viewport.canvas) return
    host.focus({ preventScroll: true })
    const p = point(e)
    if (e.pointerType === 'touch') {
      touches.set(e.pointerId, { x: p.x, y: p.y })
      if (touches.size === 2) {
        // Second finger: abort the one-finger gesture and start pinch/pan.
        void core.cancel('capture-lost')
        pinch = pinchState()
        return
      }
      if (touches.size > 2) return
    }
    viewport.canvas.setPointerCapture?.(e.pointerId)
    if (e.button === 1 || (e.button === 0 && spaceDown)) {
      panFrom = { x: p.x, y: p.y }
      e.preventDefault()
      return
    }
    void core.pointerDown(p)
  })
  on(host, 'pointermove', (e) => {
    const p = point(e)
    if (e.pointerType === 'touch' && touches.has(e.pointerId)) {
      touches.set(e.pointerId, { x: p.x, y: p.y })
      if (pinch && touches.size === 2) {
        const now = pinchState()
        if (now) {
          viewport.panBy(now.cx - pinch.cx, now.cy - pinch.cy)
          if (pinch.d > 0 && now.d > 0) viewport.zoomAt(now.cx, now.cy, now.d / pinch.d)
          pinch = now
        }
        return
      }
    }
    if (panFrom) {
      viewport.panBy(p.x - panFrom.x, p.y - panFrom.y)
      panFrom = { x: p.x, y: p.y }
      return
    }
    void core.pointerMove(p)
  })
  const up = (e: PointerEvent): void => {
    const p = point(e)
    if (e.pointerType === 'touch') {
      touches.delete(e.pointerId)
      if (pinch) {
        if (touches.size < 2) pinch = null
        return
      }
    }
    viewport.canvas.releasePointerCapture?.(e.pointerId)
    if (panFrom) {
      panFrom = null
      return
    }
    void core.pointerUp(p)
  }
  on(host, 'pointerup', up)
  on(host, 'pointercancel', (e) => {
    touches.delete(e.pointerId)
    pinch = null
    panFrom = null
    void core.cancel('capture-lost')
  })
  on(host, 'lostpointercapture', () => {
    // Capture is released normally on pointerup; losing it with a button held aborts.
    if (core.pressed) void core.cancel('capture-lost')
  })
  on(host, 'pointerleave', (e) => {
    if (e.target === viewport.canvas && !core.pressed) core.pointerLeave()
  })
  on(host, 'dblclick', (e) => {
    if (e.target === viewport.canvas) void core.doubleClick(point(e))
  })
  on(host, 'contextmenu', (e) => {
    if (e.target === viewport.canvas) e.preventDefault()
  })
  on(
    host,
    'wheel',
    (e) => {
      if (e.target !== viewport.canvas) return
      e.preventDefault()
      const p = point(e)
      // Trackpad pinch arrives as ctrl+wheel in Chromium/Firefox/Safari.
      if (opts.wheel === 'pan' && !e.ctrlKey && !e.metaKey) {
        viewport.panBy(-e.deltaX, -e.deltaY)
      } else if (e.shiftKey && !e.ctrlKey) {
        viewport.panBy(-e.deltaY, 0)
      } else {
        core.wheel(p.x, p.y, e.deltaY, e.deltaMode)
      }
    },
    { passive: false },
  )
  on(host, 'keydown', (e) => {
    if (isEditableTarget(e.target)) return
    if (e.key === ' ' && !spaceDown) {
      spaceDown = true
      e.preventDefault()
      return
    }
    const k: InputKey = {
      key: e.key,
      code: e.code,
      shift: e.shiftKey,
      mod: mac ? e.metaKey : e.ctrlKey,
      alt: e.altKey,
      repeat: e.repeat,
    }
    if (e.isComposing) return
    if (core.keyDown(k)) e.preventDefault()
  })
  on(host, 'keyup', (e) => {
    if (e.key === ' ') spaceDown = false
  })
  on(window, 'blur', () => {
    spaceDown = false
    panFrom = null
    touches.clear()
    pinch = null
    if (core.pressed) void core.cancel('blur')
  })
  return () => {
    for (const f of off.splice(0)) f()
  }
}

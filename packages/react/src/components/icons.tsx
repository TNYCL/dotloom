/** Minimal inline icons (24×24, stroke = currentColor). Decorative: aria-hidden. */

import type { ReactNode } from 'react'

const P: Record<string, ReactNode> = {
  select: <path d="M5 3l12 9-5 1 3 6-2 1-3-6-4 4z" />,
  pan: <path d="M12 3v18M3 12h18M12 3l-3 3M12 3l3 3M12 21l-3-3M12 21l3-3M3 12l3-3M3 12l3 3M21 12l-3-3M21 12l-3 3" />,
  line: <path d="M5 19L19 5" />,
  polyline: <path d="M4 18l5-9 5 6 6-10" />,
  rect: <rect x="4" y="6" width="16" height="12" />,
  circle: <circle cx="12" cy="12" r="7" />,
  arc: <path d="M4 17a9 9 0 0 1 16 0" />,
  path: <path d="M4 18c3-12 13 0 16-12" />,
  text: <path d="M6 6h12M12 6v13M9 19h6" />,
  dimension: <path d="M4 8v8M20 8v8M4 12h16M7 10l-3 2 3 2M17 10l3 2-3 2" />,
  move: <path d="M12 3v18M3 12h18M9 6l3-3 3 3M9 18l3 3 3-3M6 9l-3 3 3 3M18 9l3 3-3 3" />,
  rotate: <path d="M19 12a7 7 0 1 1-2-5M19 4v4h-4" />,
  scale: <path d="M4 20V10h10v10zM14 10l6-6M15 4h5v5" />,
  split: <path d="M4 12h6M14 12h6M12 6v12" />,
  trim: <path d="M4 12h9M16 6l-4 12M18 12h2" />,
  extend: <path d="M4 12h12M16 12l-3-3M16 12l-3 3M20 5v14" />,
  undo: <path d="M9 14L4 9l5-5M4 9h10a6 6 0 0 1 0 12h-3" />,
  redo: <path d="M15 14l5-5-5-5M20 9H10a6 6 0 0 0 0 12h3" />,
  palette: <path d="M4 6h16M4 12h16M4 18h10" />,
  eye: <path d="M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12zM12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z" />,
  lock: <path d="M6 11h12v9H6zM8 11V8a4 4 0 0 1 8 0v3" />,
  unlock: <path d="M6 11h12v9H6zM8 11V8a4 4 0 0 1 7.5-2" />,
  trash: <path d="M5 7h14M10 7V4h4v3M7 7l1 13h8l1-13" />,
  plus: <path d="M12 5v14M5 12h14" />,
}

export function Icon(props: { name: string; size?: number }): ReactNode {
  const p = P[props.name]
  const size = props.size ?? 18
  if (!p) {
    return (
      <span aria-hidden="true" style={{ fontWeight: 600, width: size, textAlign: 'center' }}>
        {props.name.slice(0, 1).toUpperCase()}
      </span>
    )
  }
  return (
    <svg
      aria-hidden="true"
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.8}
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      {p}
    </svg>
  )
}

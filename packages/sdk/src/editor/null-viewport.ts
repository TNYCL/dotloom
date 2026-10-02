/**
 * A viewport that draws nothing. Used when no GPU backend is available (the
 * editor still edits documents through panels and keyboard) and in tests.
 */

import { type Camera, fitBounds, isValidCamera, screenToWorld, type ViewSize, worldToScreen } from '../camera.js'
import type { Aabb, EntityId, Point } from '../types.js'
import { DEFAULT_GRID, type GridSettings, type OverlayState, type ViewportLike } from '../viewport.js'

export class NullViewport implements ViewportLike {
  size: ViewSize
  camera: Camera = { center: [0, 0], scale: 1 }
  grid: GridSettings = { ...DEFAULT_GRID }
  overlay: OverlayState = {}
  hover: EntityId | null = null

  constructor(size: ViewSize = { width: 800, height: 600, dpr: 1 }) {
    this.size = size
  }

  setCamera(c: Camera): void {
    if (isValidCamera(c)) this.camera = c
  }

  screenToWorld(x: number, y: number): Point {
    return screenToWorld(this.camera, this.size, x, y)
  }

  worldToScreen(p: Point): Point {
    return worldToScreen(this.camera, this.size, p)
  }

  setOverlay(o: OverlayState): void {
    this.overlay = o
  }

  setHover(id: EntityId | null): void {
    this.hover = id
  }

  setGrid(g: Partial<GridSettings>): void {
    this.grid = { ...this.grid, ...g }
  }

  async fit(bounds?: Aabb | null, margin = 32): Promise<void> {
    if (bounds) this.camera = fitBounds(this.size, bounds, margin)
  }

  requestRender(): void {}
}

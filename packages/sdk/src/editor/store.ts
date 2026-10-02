/**
 * Minimal external store (`subscribe` / `getSnapshot`), compatible with React's
 * `useSyncExternalStore` and usable from any framework. Snapshots are immutable;
 * `set` replaces the snapshot only when a field actually changed.
 */
export class Store<T extends object> {
  private value: T
  private readonly subs = new Set<() => void>()

  constructor(initial: T) {
    this.value = initial
  }

  readonly getSnapshot = (): T => this.value

  readonly subscribe = (cb: () => void): (() => void) => {
    this.subs.add(cb)
    return () => {
      this.subs.delete(cb)
    }
  }

  /** Shallow-merge `patch`; notifies only when something changed. */
  set(patch: Partial<T>): void {
    let changed = false
    for (const k of Object.keys(patch) as (keyof T)[]) {
      if (!Object.is(this.value[k], patch[k])) {
        changed = true
        break
      }
    }
    if (!changed) return
    this.value = { ...this.value, ...patch }
    for (const cb of [...this.subs]) cb()
  }
}

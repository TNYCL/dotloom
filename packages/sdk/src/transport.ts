/**
 * Transports between the SDK client and an engine host.
 *
 * * `WorkerTransport` (browser default): the engine runs in a module Web Worker.
 * * `InlineTransport`: the engine runs in the calling thread (Node, tests, tools).
 *   Messages are still delivered asynchronously, so client code behaves the same.
 */

import type { EngineFactory } from './host.js'
import { EngineHost } from './host.js'
import type { ClientMessage, HostMessage } from './protocol.js'

export interface Transport {
  send(msg: ClientMessage, transfer?: Transferable[]): void
  onMessage(cb: (msg: HostMessage) => void): void
  onCrash(cb: (reason: string) => void): void
  terminate(): void
}

export class WorkerTransport implements Transport {
  private readonly worker: Worker
  private crashCb: ((reason: string) => void) | null = null

  constructor(workerUrl?: string | URL) {
    // The literal `new URL(..., import.meta.url)` lets bundlers emit the worker.
    this.worker =
      workerUrl === undefined
        ? new Worker(new URL('./worker.js', import.meta.url), { type: 'module', name: 'dotloom-engine' })
        : new Worker(workerUrl, { type: 'module', name: 'dotloom-engine' })
    this.worker.addEventListener('error', (ev) => {
      ev.preventDefault()
      this.crashCb?.(ev.message || 'worker error')
    })
    this.worker.addEventListener('messageerror', () => this.crashCb?.('message could not be deserialized'))
  }

  send(msg: ClientMessage, transfer?: Transferable[]): void {
    this.worker.postMessage(msg, transfer ?? [])
  }

  onMessage(cb: (msg: HostMessage) => void): void {
    this.worker.addEventListener('message', (ev: MessageEvent<HostMessage>) => cb(ev.data))
  }

  onCrash(cb: (reason: string) => void): void {
    this.crashCb = cb
  }

  terminate(): void {
    this.worker.terminate()
  }
}

export class InlineTransport implements Transport {
  private readonly host: EngineHost
  private cb: ((msg: HostMessage) => void) | null = null
  private closed = false

  constructor(factory: EngineFactory, stepBudget?: number) {
    this.host = new EngineHost(
      (msg) => {
        // Deliver asynchronously, like postMessage.
        queueMicrotask(() => {
          if (!this.closed) this.cb?.(msg)
        })
      },
      factory,
      stepBudget,
    )
  }

  send(msg: ClientMessage): void {
    // Structured-clone semantics are not needed in-thread; dispatch on a microtask.
    queueMicrotask(() => {
      if (!this.closed) this.host.receive(msg)
    })
  }

  onMessage(cb: (msg: HostMessage) => void): void {
    this.cb = cb
  }

  onCrash(_cb: (reason: string) => void): void {
    // In-thread crashes surface as `crashed` protocol messages.
  }

  terminate(): void {
    this.closed = true
  }
}

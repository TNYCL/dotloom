import { expect, test } from '@playwright/test'

// Environment baseline for DL-PERF-2: the same rAF interval measurement with a
// plain WebGL2 animation (one triangle, no Dotloom) on probe.html. If this drops
// frames too, the browser/environment does — not the renderer.

function pct(xs: number[], p: number): number {
  const s = [...xs].sort((a, b) => a - b)
  return s[Math.min(s.length - 1, Math.max(0, Math.ceil((p / 100) * s.length) - 1))] ?? Number.NaN
}

test('plain WebGL2 rAF baseline', async ({ page }, info) => {
  test.skip(info.project.metadata.backend !== 'webgl2', 'WebGL2 projects only')
  await page.goto('./probe.html')
  const intervals = await page.evaluate(async () => {
    const canvas = document.createElement('canvas')
    canvas.width = 1920
    canvas.height = 1080
    canvas.style.cssText = 'position:fixed;inset:0;width:100vw;height:100vh'
    document.body.append(canvas)
    const gl = canvas.getContext('webgl2')
    if (!gl) throw new Error('no webgl2')
    const vs = gl.createShader(gl.VERTEX_SHADER) as WebGLShader
    gl.shaderSource(
      vs,
      '#version 300 es\nin vec2 p; uniform float t; void main(){gl_Position=vec4(p+vec2(sin(t),0.)*.2,0.,1.);}',
    )
    gl.compileShader(vs)
    const fs = gl.createShader(gl.FRAGMENT_SHADER) as WebGLShader
    gl.shaderSource(fs, '#version 300 es\nprecision mediump float; out vec4 c; void main(){c=vec4(.2,.4,.8,1.);}')
    gl.compileShader(fs)
    const prog = gl.createProgram() as WebGLProgram
    gl.attachShader(prog, vs)
    gl.attachShader(prog, fs)
    gl.linkProgram(prog)
    gl.useProgram(prog)
    const buf = gl.createBuffer()
    gl.bindBuffer(gl.ARRAY_BUFFER, buf)
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-0.5, -0.5, 0.5, -0.5, 0, 0.5]), gl.STATIC_DRAW)
    gl.enableVertexAttribArray(0)
    gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0)
    const t = gl.getUniformLocation(prog, 't')
    const stamps: number[] = []
    await new Promise<void>((resolve) => {
      const step = (now: number) => {
        stamps.push(now)
        gl.clearColor(1, 1, 1, 1)
        gl.clear(gl.COLOR_BUFFER_BIT)
        gl.uniform1f(t, now / 300)
        gl.drawArrays(gl.TRIANGLES, 0, 3)
        if (stamps.length > 660) resolve()
        else requestAnimationFrame(step)
      }
      requestAnimationFrame(step)
    })
    return stamps.slice(61).map((s, i) => s - (stamps[i + 60] ?? s))
  })
  const summary = { p50: pct(intervals, 50), p95: pct(intervals, 95), dropped: intervals.filter((d) => d > 25).length }
  console.log(info.project.name, JSON.stringify(summary))
  expect(intervals.length).toBe(600)
})

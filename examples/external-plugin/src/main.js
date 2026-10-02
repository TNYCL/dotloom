import { DotloomEditor } from '@dotloom/react'
import '@dotloom/react/styles.css'
import { createElement } from 'react'
import { createRoot } from 'react-dom/client'
import { acmePlugin } from './plugin.js'

const root = document.getElementById('root')
if (root) {
  createRoot(root).render(
    createElement(DotloomEditor, {
      plugins: [acmePlugin],
      autosave: false,
      name: 'tables.dotl',
      onReady: (h) => {
        window.dotloom = h
      },
    }),
  )
}

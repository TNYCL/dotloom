import { defineConfig } from 'vitepress'

// DOTLOOM_BASE=/dotloom/ on GitHub Pages.
const base = process.env.DOTLOOM_BASE ?? '/'

export default defineConfig({
  base,
  title: 'Dotloom',
  description: 'Open-source framework for constraint-driven 2D editors',
  lang: 'en-US',
  cleanUrls: false,
  lastUpdated: false,
  ignoreDeadLinks: [/^\/(playground|examples|api)\//, /^\.\.\/(playground|examples|api)\//],
  head: [['meta', { name: 'theme-color', content: '#0a66d9' }]],
  themeConfig: {
    nav: [
      { text: 'Guide', link: '/guide/getting-started' },
      { text: 'Playground', link: `${base}playground/`, target: '_self' },
      { text: 'Examples', link: '/guide/examples' },
      { text: 'API', link: '/guide/api' },
      { text: 'GitHub', link: 'https://github.com/TNYCL/dotloom' },
    ],
    sidebar: [
      {
        text: 'Introduction',
        items: [
          { text: 'Getting started', link: '/guide/getting-started' },
          { text: 'Architecture', link: '/guide/architecture' },
          { text: 'Examples', link: '/guide/examples' },
        ],
      },
      {
        text: 'Using Dotloom',
        items: [
          { text: 'Documents and commands', link: '/guide/documents' },
          { text: 'Constraints', link: '/guide/constraints' },
          { text: 'Tools and input', link: '/guide/tools' },
          { text: 'Plugins', link: '/guide/plugins' },
          { text: 'React editor', link: '/guide/react' },
          { text: 'Storage and files', link: '/guide/storage' },
        ],
      },
      {
        text: 'Reference',
        items: [
          { text: '.dotl file format', link: '/guide/file-format' },
          { text: 'SVG, DXF and PNG', link: '/guide/formats' },
          { text: 'Command-line tool', link: '/guide/cli' },
          { text: 'API reference', link: '/guide/api' },
        ],
      },
      {
        text: 'Operations',
        items: [
          { text: 'Platforms and browsers', link: '/guide/platforms' },
          { text: 'Performance', link: '/guide/performance' },
          { text: 'Versions and migration', link: '/guide/versioning' },
          { text: 'Troubleshooting', link: '/guide/troubleshooting' },
        ],
      },
    ],
    socialLinks: [{ icon: 'github', link: 'https://github.com/TNYCL/dotloom' }],
    footer: { message: 'MIT OR Apache-2.0', copyright: 'Dotloom contributors' },
    search: { provider: 'local' },
  },
})

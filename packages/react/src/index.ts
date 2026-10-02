/**
 * `@dotloom/react` — optional React editor for Dotloom.
 *
 * Use `<DotloomEditor>` for the complete reference editor, or compose your own
 * UI from the panels and hooks inside an `EditorProvider`. Import the styles once:
 * `import '@dotloom/react/styles.css'`.
 */

export { type Action, buildActions, type FileHooks, openFileAction } from './actions.js'
export { CommandPalette, matchScore } from './components/CommandPalette.js'
export { ConstraintsPanel, ruleEntities } from './components/ConstraintsPanel.js'
export { ErrorBanner, MissingPluginsBanner, Modal, RecoveryDialog, type UiError } from './components/Dialogs.js'
export { formatValue, Inspector, parseValue } from './components/Inspector.js'
export { Icon } from './components/icons.js'
export { LayersPanel } from './components/LayersPanel.js'
export { ObjectsPanel } from './components/ObjectsPanel.js'
export { PluginPanels } from './components/PluginPanels.js'
export { StatusBar } from './components/StatusBar.js'
export { Toolbar } from './components/Toolbar.js'
export {
  type EditorHandle,
  EditorProvider,
  useAnalysis,
  useAutosaveState,
  useDocument,
  useEditor,
  useEditorState,
  useEntityInfo,
  usePluginTypes,
  usePointer,
  useRevisionQuery,
} from './context.js'
export { DotloomEditor, type DotloomEditorProps, type ThemeMode } from './DotloomEditor.js'
export {
  en,
  I18nProvider,
  LOCALES,
  type MessageKey,
  type Messages,
  makeTranslate,
  type Translate,
  tr,
  useT,
} from './i18n.js'

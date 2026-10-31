import { useEffect, useRef } from 'react'
import * as monaco from 'monaco-editor/editor/editor.api.js'
import 'monaco-editor/features/codeEditor/register.js'
import 'monaco-editor/features/tokenization/register.js'
import 'monaco-editor/features/bracketMatching/register.js'
import 'monaco-editor/features/clipboard/register.js'
import 'monaco-editor/features/find/register.js'
import 'monaco-editor/languages/definitions/cpp/register.js'
import EditorWorker from 'monaco-editor/editor/editor.worker.js?worker'

(self as typeof self & { MonacoEnvironment: monaco.Environment }).MonacoEnvironment = { getWorker: () => new EditorWorker() }

type Props = {
  initialValue: string
  filename: string
  readOnly: boolean
  onChange: (content: string) => void
  breakpoints: number[]
  currentLine?: number
  onBreakpoint: (line: number) => void
}

export default function Editor({ initialValue, filename, readOnly, onChange, breakpoints, currentLine, onBreakpoint }: Props) {
  const container = useRef<HTMLDivElement>(null)
  const instance = useRef<monaco.editor.IStandaloneCodeEditor | null>(null)
  const changed = useRef(onChange)
  changed.current = onChange
  const toggle = useRef(onBreakpoint)
  toggle.current = onBreakpoint
  const decorations = useRef<monaco.editor.IEditorDecorationsCollection | null>(null)

  useEffect(() => {
    const model = monaco.editor.createModel(initialValue, filename.endsWith('.txt') ? 'plaintext' : 'c')
    const editor = monaco.editor.create(container.current!, {
      model, readOnly, automaticLayout: true, minimap: { enabled: false },
      fontSize: 14, lineNumbers: 'on', scrollBeyondLastLine: false, glyphMargin: true,
      ariaLabel: `Edit ${filename}`, quickSuggestions: false,
      wordBasedSuggestions: 'off', suggestOnTriggerCharacters: false,
    })
    instance.current = editor
    decorations.current = editor.createDecorationsCollection()
    const click = editor.onMouseDown(event => {
      if (event.target.type === monaco.editor.MouseTargetType.GUTTER_GLYPH_MARGIN && event.target.position) toggle.current(event.target.position.lineNumber)
    })
    const subscription = model.onDidChangeContent(() => {
      changed.current(model.getValue(monaco.editor.EndOfLinePreference.TextDefined, true))
    })
    return () => { click.dispose(); subscription.dispose(); editor.dispose(); model.dispose(); instance.current = null; decorations.current = null }
    // The parent remounts this component only when opening or reloading a file.
  }, [])

  useEffect(() => { instance.current?.updateOptions({ readOnly }) }, [readOnly])
  useEffect(() => {
    const marks: monaco.editor.IModelDeltaDecoration[] = breakpoints.map(line => ({ range: new monaco.Range(line, 1, line, 1), options: { glyphMarginClassName: 'breakpoint-marker', glyphMarginHoverMessage: { value: 'Breakpoint' } } }))
    if (currentLine) {
      marks.push({ range: new monaco.Range(currentLine, 1, currentLine, 1), options: { isWholeLine: true, className: 'paused-line' } })
      instance.current?.revealLineInCenterIfOutsideViewport(currentLine)
    }
    decorations.current?.set(marks)
  }, [breakpoints, currentLine])
  return <div className="code-editor" ref={container} />
}

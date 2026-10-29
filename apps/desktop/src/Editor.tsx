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
}

export default function Editor({ initialValue, filename, readOnly, onChange }: Props) {
  const container = useRef<HTMLDivElement>(null)
  const instance = useRef<monaco.editor.IStandaloneCodeEditor | null>(null)
  const changed = useRef(onChange)
  changed.current = onChange

  useEffect(() => {
    const model = monaco.editor.createModel(initialValue, filename.endsWith('.txt') ? 'plaintext' : 'c')
    const editor = monaco.editor.create(container.current!, {
      model, readOnly, automaticLayout: true, minimap: { enabled: false },
      fontSize: 14, lineNumbers: 'on', scrollBeyondLastLine: false,
      ariaLabel: `Edit ${filename}`, quickSuggestions: false,
      wordBasedSuggestions: 'off', suggestOnTriggerCharacters: false,
    })
    instance.current = editor
    const subscription = model.onDidChangeContent(() => {
      changed.current(model.getValue(monaco.editor.EndOfLinePreference.TextDefined, true))
    })
    return () => { subscription.dispose(); editor.dispose(); model.dispose(); instance.current = null }
    // The parent remounts this component only when opening or reloading a file.
  }, [])

  useEffect(() => { instance.current?.updateOptions({ readOnly }) }, [readOnly])
  return <div className="code-editor" ref={container} />
}

import { useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'
import type { CompileState, Preferences, Settings, Tool } from './types'

type Props = {
  settings: Settings; compilation: CompileState; locked: boolean; ready: boolean
  perform: (action: () => Promise<void>, label?: string) => Promise<void>
  onCompilation: (value: CompileState) => void; onApply: (preferences: Preferences) => Promise<void>
}

export default function SettingsPanel({ settings, compilation, locked, ready, perform, onCompilation, onApply }: Props) {
  const [draft, setDraft] = useState(settings.preferences)
  const [tool, setTool] = useState<Tool>('GCC')
  const [toolPath, setToolPath] = useState('')
  function browse(key: keyof Preferences) {
    void perform(async () => {
      const path = await open({ directory: true, multiple: false, title: 'Choose directory' })
      if (path !== null) setDraft(value => ({ ...value, [key]: path }))
    })
  }
  return <div className="settings-page">
    <section className="settings"><h2>Workspace and temporary files</h2><p className="hint">Courses, exercises, and projects are saved in your workspace. Changing it keeps existing files in place.</p>
      <fieldset disabled={locked}>{(['workspace_root', 'temp_root'] as const).map(key => <div className="path-setting" key={key}><label htmlFor={key}>{key === 'workspace_root' ? 'Workspace folder' : 'Temporary folder'}</label><div className="actions"><input id={key} value={draft[key] ?? ''} placeholder={key === 'workspace_root' ? settings.default_workspace_root : settings.default_temp_root} onChange={event => setDraft(value => ({ ...value, [key]: event.target.value || null }))} /><button onClick={() => browse(key)}>Browse</button><button onClick={() => setDraft(value => ({ ...value, [key]: null }))}>Use default</button></div></div>)}<button onClick={() => void perform(() => onApply(draft))}>Apply folders</button></fieldset>
      <p className="resolved">Workspace: {settings.resolved_workspace_root ?? 'Unavailable'}</p><p className="resolved">Temporary files: {settings.resolved_temp_root ?? 'Unavailable'}</p>
    </section>
    <section className="toolchain-panel"><h2>Compiler and debugger</h2><p>Choose installed tools. Your choices are saved and checked again when the app starts.</p><p className="resolved">GCC: {compilation.gcc?.path ?? 'Not selected'}</p><p className="resolved">GDB: {compilation.gdb?.path ?? 'Not selected'}</p>
      <fieldset disabled={locked || !ready}><div className="actions"><button onClick={() => void perform(async () => { onCompilation(await invoke<CompileState>('discover_toolchains')) }, 'Checking installed tools…')}>Find installed tools</button><select aria-label="Tool" value={tool} onChange={event => setTool(event.target.value as Tool)}><option>GCC</option><option>GDB</option></select><input aria-label="Tool executable path" value={toolPath} onChange={event => setToolPath(event.target.value)} placeholder="Absolute path to the executable" /><button onClick={() => void perform(async () => { const path = await open({ directory: false, multiple: false, title: `Choose ${tool}` }); if (path) setToolPath(path) })}>Browse</button><button disabled={!toolPath} onClick={() => void perform(async () => { onCompilation(await invoke<CompileState>('validate_toolchain', { tool, path: toolPath })) }, `Checking ${tool}…`)}>Check tool</button></div></fieldset>
      <ul className="tools">{compilation.candidates.map(candidate => <li key={candidate.id}><div><strong>{candidate.tool}</strong><p>{candidate.path}</p><p>{candidate.version}</p>{candidate.error && <p className="tool-error">{candidate.error}</p>}{candidate.details && <details><summary>Validation details</summary><pre>{candidate.details}</pre></details>}</div><button disabled={locked || !candidate.usable} onClick={() => void perform(async () => { onCompilation(await invoke<CompileState>('select_toolchain', { id: candidate.id })) })}>Use {candidate.tool}</button></li>)}</ul>
    </section>
  </div>
}

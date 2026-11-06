import { useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'
import type { Tool } from './types'

type Assessment = {
  root: string
  gcc_version: string | null
  gdb_version: string | null
  selected: Tool[]
  preview: { repository: string; name: string; version: string; installed_version: string | null }[]
  preview_error: string | null
  installation_block: string
}
type Props = {
  locked: boolean
  ready: boolean
  perform: (action: () => Promise<void>, label?: string) => Promise<void>
}

export default function ToolchainSetup({ locked, ready, perform }: Props) {
  const [root, setRoot] = useState('')
  const [assessment, setAssessment] = useState<Assessment | null>(null)
  const [tools, setTools] = useState<Tool[]>([])
  const [error, setError] = useState('')
  // Changing either input invalidates the old preview immediately.
  const [previewCurrent, setPreviewCurrent] = useState(false)
  function changeRoot(value: string) {
    setRoot(value); setAssessment(null); setTools([]); setPreviewCurrent(false); setError('')
  }
  function check(selected: Tool[] | null) {
    void perform(async () => {
      setAssessment(null); setPreviewCurrent(false); setError('')
      try {
        const result = await invoke<Assessment>('assess_toolchain_setup', { root: root.trim() || null, tools: selected })
        setAssessment(result); setRoot(result.root); setTools(result.selected); setPreviewCurrent(true)
      } catch (cause) { setError(String(cause)) }
    }, 'Checking MSYS2 and installed tools…')
  }
  return <section className="toolchain-panel">
    <h2>Toolchain setup (Windows)</h2>
    <p>Use working tools already on your computer first. MSYS2 UCRT64 checks are read-only; Linux setup will be added later.</p>
    <p>Need MSYS2? Copy the official guide address into your browser: <code>https://www.msys2.org/</code>. For manual updates: <code>https://www.msys2.org/docs/updating/</code>. Complete setup there, then return here and check again.</p>
    <details><summary>Manual GCC / GDB installation</summary><p>After following the official update instructions, open the MSYS2 UCRT64 terminal. Run the command for each tool you need and review pacman's proposed changes before confirming. Manual package operations may change existing packages.</p><pre>pacman -S --needed mingw-w64-ucrt-x86_64-gcc{'\n'}pacman -S --needed mingw-w64-ucrt-x86_64-gdb</pre><p>Return here and check again, then explicitly choose a validated tool.</p></details>
    <fieldset disabled={locked || !ready}>
      <label htmlFor="msys2-root">MSYS2 directory</label>
      <div className="actions"><input id="msys2-root" value={root} placeholder="C:\msys64 (default)" onChange={event => changeRoot(event.target.value)} />
        <button onClick={() => void perform(async () => { const path = await open({ directory: true, multiple: false, title: 'Choose MSYS2 directory' }); if (path) changeRoot(path) })}>Browse</button>
        <button onClick={() => changeRoot('')}>Use default</button>
        <button onClick={() => check(null)}>{error ? 'Retry check' : 'Check MSYS2'}</button>
      </div>
      {assessment && <>
        <p>GCC package: {assessment.gcc_version ?? 'Not installed'}</p>
        <p>GDB package: {assessment.gdb_version ?? 'Not installed'}</p>
        <p className="hint">Tools are checked by default only when neither a working tool in CodeLearn nor the package in this MSYS2 is known. Choose GCC and GDB individually to inspect a preview.</p>
        {(['GCC', 'GDB'] as const).map(tool => <label className="check" key={tool}><input type="checkbox" checked={tools.includes(tool)} onChange={event => { setTools(value => event.target.checked ? [...value, tool] : value.filter(item => item !== tool)); setPreviewCurrent(false) }} />{tool}</label>)}
        <button onClick={() => check(tools)}>Refresh package preview</button>
      </>}
    </fieldset>
    {error && <div role="alert" className="tool-error"><p>MSYS2 could not be checked. Review the directory and official setup instructions, then retry.</p><details><summary>Technical details</summary><pre>{error}</pre></details></div>}
    {assessment && <>
      <p className="tool-error" role="status">{assessment.installation_block}</p>
      {previewCurrent ? <>
        {assessment.preview_error && <div role="alert" className="tool-error"><p>Preview unavailable. Resolve the problem in MSYS2, then retry the check.</p><details><summary>Technical details</summary><pre>{assessment.preview_error}</pre></details></div>}
        {!assessment.preview_error && <>
          <p className="hint">Advisory package preview only. It does not include a reliable list of removals or replacements and cannot authorize installation.</p>
          {assessment.preview.length ? <table><caption>Requested tools and resolved dependencies</caption><thead><tr><th>Repository / package</th><th>Version</th><th>Assessment</th></tr></thead><tbody>{assessment.preview.map(pkg => <tr key={pkg.name}><td>{pkg.repository} / {pkg.name}</td><td>{pkg.version}</td><td>{pkg.installed_version ? `Existing package (${pkg.installed_version}) — changes prohibited` : 'New package — would require confirmation'}</td></tr>)}</tbody></table> : <p>No package additions were listed.</p>}
        </>}
      </> : <p role="status">Selection changed. Refresh the preview to see the selected packages.</p>}
      <p>Detected GCC/GDB executables are checked using the existing tool validation. Review the candidates above and explicitly choose Use GCC / Use GDB. Package presence alone does not mean a tool is usable.</p>
    </>}
  </section>
}

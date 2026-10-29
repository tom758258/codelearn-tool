import { useEffect, useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { invoke } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { open } from '@tauri-apps/plugin-dialog'
import Editor from './Editor'
import codelearnIcon from '../src-tauri/icons/codelearn-icon.png'
import './styles.css'

type Area = 'Imported' | 'Authored'
type Preferences = { workspace_root: string | null; temp_root: string | null }
type Settings = {
  preferences: Preferences
  default_workspace_root: string
  default_temp_root: string
  resolved_workspace_root: string | null
  resolved_temp_root: string | null
  error: string | null
}
type Entry = { name: string; relative_path: string; is_directory: boolean; blocked: boolean }
type Listing = { path: string; displayPath: string; entries: Entry[]; selected: string | null }
type DirectoryEntries = { display_path: string; entries: Entry[] }
type Document = { relative_path: string; display_path: string; content: string }
type OpenDocument = Document & { value: string; generation: number }
type Targets = { primary: string | null; additional: string[]; includes: string[] }
type Tool = 'GCC' | 'GDB'
type SelectedTool = { path: string; version: string | null }
type Candidate = { id: number; tool: Tool; path: string; origin: string; version: string | null; usable: boolean; error: string | null; details: string }
type CompileState = {
  targets: Targets
  candidates: Candidate[]
  gcc: SelectedTool | null
  gdb: SelectedTool | null
  build_valid: boolean
  output: { success: boolean; exit_code: number | null; stdout: string; stderr: string; error: string | null; cleanup_errors: string[] } | null
  cleanup_errors: string[]
}
type Choice = 'save' | 'discard' | 'cancel'
const emptyListing = (): Listing => ({ path: '', displayPath: '', entries: [], selected: null })
const emptyCompile = (): CompileState => ({ targets: { primary: null, additional: [], includes: [] }, candidates: [], gcc: null, gdb: null, build_valid: false, output: null, cleanup_errors: [] })
const isDirty = (document: OpenDocument | null) => document !== null && document.value !== document.content
// Display undecodable native filename bytes without changing their command identity.
const displayPath = (path: string) => new TextDecoder().decode(Uint8Array.from(
  path.match(/%[0-9a-f]{2}|./gi) ?? [], part => part.startsWith('%') ? parseInt(part.slice(1), 16) : part.charCodeAt(0),
))

function App() {
  const [settings, setSettings] = useState<Settings | null>(null)
  const [draft, setDraft] = useState<Preferences>({ workspace_root: null, temp_root: null })
  const [imported, setImported] = useState<Listing>(emptyListing)
  const [authored, setAuthored] = useState<Listing>(emptyListing)
  const [filename, setFilename] = useState('')
  const [busy, setBusy] = useState(true)
  const [activity, setActivity] = useState('Loading…')
  const [error, setError] = useState('')
  const [message, setMessage] = useState('')
  const [document, setDocument] = useState<OpenDocument | null>(null)
  const [compilation, setCompilation] = useState<CompileState>(emptyCompile)
  const [tool, setTool] = useState<Tool>('GCC')
  const [toolPath, setToolPath] = useState('')
  const [question, setQuestion] = useState<string | null>(null)
  const running = useRef(true)
  const currentDocument = useRef<OpenDocument | null>(null)
  const editorUpdates = useRef<Promise<void>>(Promise.resolve())
  const dialog = useRef<HTMLDialogElement>(null)
  const answer = useRef<((choice: Choice) => void) | null>(null)
  const closeAction = useRef<() => void>(() => {})
  const allowClose = useRef(false)
  const ready = settings !== null && settings.error === null
  const dirty = isDirty(document)
  const selected = authored.entries.find(entry => entry.relative_path === authored.selected)
  const selectedSource = selected && !selected.blocked && !selected.is_directory && selected.name.endsWith('.c')

  function updateDocument(value: OpenDocument | null) {
    currentDocument.current = value
    setDocument(value)
  }

  async function readListing(area: Area, path: string) {
    const setListing = area === 'Imported' ? setImported : setAuthored
    const result = await invoke<DirectoryEntries>('list_workspace_entries', { area, relativePath: path })
    setListing({ path, displayPath: result.display_path, entries: result.entries, selected: null })
  }

  async function refresh(importedPath = imported.path, authoredPath = authored.path) {
    await readListing('Imported', importedPath)
    await readListing('Authored', authoredPath)
  }

  async function syncEditor() {
    await editorUpdates.current
    const current = currentDocument.current
    if (current) await invoke('set_editor_dirty', { relativePath: current.relative_path, dirty: isDirty(current) })
  }

  async function perform(action: () => Promise<void>, label = 'Working…') {
    if (running.current) return
    running.current = true
    setBusy(true)
    setActivity(label)
    setError('')
    setMessage('')
    try { await syncEditor(); await action() }
    catch (cause) { setError(String(cause)) }
    finally {
      try { setCompilation(await invoke<CompileState>('get_compile_state')) }
      catch (cause) { setError(previous => previous || String(cause)) }
      running.current = false
      setBusy(false)
      setActivity('')
    }
  }

  useEffect(() => {
    async function load() {
      try {
        const current = await invoke<Settings>('get_workspace_settings')
        setSettings(current)
        setDraft(current.preferences)
        if (!current.error) await refresh('', '')
        setCompilation(await invoke<CompileState>('get_compile_state'))
      } catch (cause) { setError(String(cause)) }
      finally { running.current = false; setBusy(false); setActivity('') }
    }
    void load()
    let disposed = false
    const stop = getCurrentWindow().onCloseRequested(event => {
      if (allowClose.current) return
      if (running.current || isDirty(currentDocument.current)) {
        event.preventDefault()
        closeAction.current()
      }
    })
    void stop.then(unlisten => { if (disposed) unlisten() }).catch(cause => setError(String(cause)))
    return () => { disposed = true; void stop.then(unlisten => unlisten()).catch(() => {}) }
  }, [])

  useEffect(() => { if (question) dialog.current?.showModal() }, [question])

  function choose(choice: Choice) {
    dialog.current?.close()
    setQuestion(null)
    answer.current?.(choice)
    answer.current = null
  }

  async function save() {
    const current = currentDocument.current
    if (!current) return
    const saved = await invoke<Document>('save_authored_file', { relativePath: current.relative_path, content: current.value })
    updateDocument({ ...saved, value: saved.content, generation: current.generation })
    setMessage('File saved.')
  }

  async function beforeSwitch(reason: string): Promise<boolean | null> {
    if (!isDirty(currentDocument.current)) return false
    setQuestion(reason)
    const choice = await new Promise<Choice>(resolve => { answer.current = resolve })
    if (choice === 'cancel') return null
    if (choice === 'save') await save()
    return choice === 'discard'
  }

  closeAction.current = () => {
    if (running.current) { setError('Please wait for the current operation to finish before closing.'); return }
    let shouldClose = false
    void perform(async () => {
      shouldClose = await beforeSwitch('Save your changes before closing?') !== null
    }).then(async () => {
      if (shouldClose) {
        allowClose.current = true
        try { await getCurrentWindow().close() }
        catch (cause) { allowClose.current = false; setError(String(cause)) }
      }
    })
  }

  function edit(value: string) {
    const current = currentDocument.current
    if (!current || running.current) return
    const next = { ...current, value }
    updateDocument(next)
    if (isDirty(next)) setCompilation(previous => ({ ...previous, build_valid: false }))
    if (isDirty(current) !== isDirty(next)) {
      editorUpdates.current = editorUpdates.current.then(() => invoke<void>('set_editor_dirty', {
        relativePath: next.relative_path, dirty: isDirty(next),
      })).catch(cause => { setError(String(cause)) })
    }
  }

  function openFile(relativePath: string, reload = false) {
    if (!reload && currentDocument.current?.relative_path === relativePath) return
    void perform(async () => {
      const discardChanges = await beforeSwitch(reload ? 'Save your changes before reloading this file?' : 'Save your changes before opening another file?')
      if (discardChanges === null) return
      const opened = await invoke<Document>('read_authored_file', { relativePath, discardChanges })
      updateDocument({ ...opened, value: opened.content, generation: (currentDocument.current?.generation ?? 0) + 1 })
    })
  }

  async function setTargets(targets: Targets) {
    setCompilation(await invoke<CompileState>('set_compile_targets', { targets }))
  }

  function browse(key: keyof Preferences) {
    void perform(async () => {
      const path = await open({ directory: true, multiple: false, title: 'Choose directory' })
      if (path !== null) setDraft(value => ({ ...value, [key]: path }))
    })
  }

  function importSource(directory: boolean) {
    void perform(async () => {
      const source = await open({ directory, multiple: false, title: directory ? 'Import Folder' : 'Import File' })
      if (source === null) return
      await invoke('import_to_workspace', { source })
      await refresh()
      setMessage('Imported successfully. The source was preserved.')
    })
  }

  function files(area: Area, listing: Listing) {
    const setListing = area === 'Imported' ? setImported : setAuthored
    return (
      <section className="file-panel" aria-label={area}>
        <div className="panel-title"><h3>{area}</h3><span>{area === 'Imported' ? 'Original copies' : 'Your working files'}</span></div>
        <div className="directory-bar">
          <button disabled={busy || !ready || !listing.path} onClick={() => void perform(() => readListing(area, listing.path.split('/').slice(0, -1).join('/')))}>Up</button>
          <code>{area}/{listing.displayPath}</code>
        </div>
        <ul className="entries" aria-label={`${area} entries`}>
          {listing.entries.map(entry => (
            <li key={entry.relative_path}>
              <button className={listing.selected === entry.relative_path ? 'entry selected' : 'entry'}
                disabled={busy || entry.blocked || !ready}
                onClick={() => setListing(value => ({ ...value, selected: entry.relative_path }))}
                aria-pressed={listing.selected === entry.relative_path}>
                <span aria-hidden="true">{entry.blocked ? '×' : entry.is_directory ? '▸' : '·'}</span>
                <span>{entry.name}{entry.is_directory ? '/' : ''}{entry.blocked ? ' (unsupported link or file)' : ''}</span>
              </button>
              {entry.is_directory ? <button disabled={busy || !ready || entry.blocked} onClick={() => void perform(() => readListing(area, entry.relative_path))} aria-label={`Open ${entry.name}`}>Open</button>
                : area === 'Authored' && /\.(c|h|txt)$/.test(entry.name) && <button disabled={busy || !ready || entry.blocked} onClick={() => openFile(entry.relative_path)} aria-label={`Edit ${entry.name}`}>Edit</button>}
            </li>
          ))}
          {!listing.entries.length && <li className="empty">{busy ? 'Loading…' : ready ? 'No entries' : 'Workspace is not ready'}</li>}
        </ul>
        {area === 'Imported' ? (
          <div className="actions">
            <button disabled={busy || !ready} onClick={() => importSource(false)}>Import File</button>
            <button disabled={busy || !ready} onClick={() => importSource(true)}>Import Folder</button>
            <button disabled={busy || !ready || !listing.selected} onClick={() => void perform(async () => {
              await invoke('copy_to_authored', { relativePath: listing.selected })
              await refresh()
              setMessage('Copied to Authored, preserving the relative path.')
            })}>Copy to Authored</button>
          </div>
        ) : (
          <form onSubmit={event => {
            event.preventDefault()
            void perform(async () => {
              await invoke('create_authored_file', { parentRelativePath: listing.path, filename })
              setFilename('')
              await refresh()
              setMessage('Empty file created in Authored.')
            })
          }}>
            <label htmlFor="filename">New File in the current folder (.c, .h, .txt)</label>
            <div className="actions"><input id="filename" value={filename} disabled={busy || !ready} placeholder="hello.c" onChange={event => setFilename(event.target.value)} /><button disabled={busy || !ready || !filename.trim()} type="submit">New File</button></div>
          </form>
        )}
      </section>
    )
  }

  const targets = compilation.targets
  return (
    <main aria-busy={busy}>
      <header><img className="mark" src={codelearnIcon} alt="" /><div><h1>CodeLearn Tool</h1><p>Your learning workspace, starting with C.</p></div></header>
      <section className="settings">
        <div className="section-title"><h2>Workspace Settings</h2><span className="status" role="status">{busy ? activity : error || !ready ? 'Needs attention' : 'Ready'}</span></div>
        <fieldset disabled={busy || settings === null}>
          {(['workspace_root', 'temp_root'] as const).map(key => {
            const workspace = key === 'workspace_root'
            const fallback = (workspace ? settings?.default_workspace_root : settings?.default_temp_root) ?? ''
            const resolved = workspace ? settings?.resolved_workspace_root : settings?.resolved_temp_root
            return <div className="path-setting" key={key}>
              <label htmlFor={key}>{workspace ? 'Learning Workspace' : 'CodeLearnTemp'}</label>
              <div className="actions"><input id={key} value={draft[key] ?? fallback} onChange={event => setDraft(value => ({ ...value, [key]: event.target.value }))} /><button onClick={() => browse(key)}>Browse</button><button onClick={() => setDraft(value => ({ ...value, [key]: null }))}>Restore Default</button></div>
              <p className="resolved">Active: <code>{resolved ?? 'Not available'}</code>{draft[key] === null ? ' · Default' : ''}</p>
            </div>
          })}
          <button className="primary" onClick={() => void perform(async () => {
            const discardChanges = await beforeSwitch('Save your changes before applying workspace settings?')
            if (discardChanges === null) return
            const current = await invoke<Settings>('apply_workspace_settings', { preferences: draft, discardChanges })
            updateDocument(null)
            setSettings(current)
            setDraft(current.preferences)
            setImported(emptyListing())
            setAuthored(emptyListing())
            await refresh('', '')
            setMessage('Settings applied. Existing workspace files and Temp contents were preserved.')
          })}>Apply</button>
        </fieldset>
        <p className="hint">Workspace and Temp must be separate. Changing paths preserves files in the old locations. Restore Default takes effect after Apply.</p>
      </section>
      {(error || settings?.error) && <p role="alert" className="error">{error || settings?.error}</p>}
      {message && <p role="status" className="success">{message}</p>}
      <div className="section-title"><h2>Workspace Files</h2><button disabled={busy || !ready} onClick={() => void perform(() => refresh())}>Refresh</button></div>
      <div className="files">{files('Imported', imported)}{files('Authored', authored)}</div>
      <section className="editor-panel">
        <div className="section-title"><h2>Editor</h2><span>{document ? dirty ? 'Unsaved changes' : 'Saved' : 'No file open'}</span></div>
        {document ? <>
          <div className="editor-toolbar"><code>Authored/{document.display_path}</code><div className="actions">
            <button disabled={busy || !dirty} onClick={() => void perform(save)}>Save</button>
            <button disabled={busy} onClick={() => openFile(document.relative_path, true)}>Reload</button>
          </div></div>
          <Editor key={document.generation} initialValue={document.content} filename={document.display_path} readOnly={busy} onChange={edit} />
          {busy && <p role="status" className="hint">{activity} Editing will be available when the operation finishes.</p>}
        </> : <p className="empty">Choose Edit beside a .c, .h or .txt file in Authored. Copy imported files to Authored first.</p>}
      </section>
      <section className="toolchain-panel">
        <div className="section-title"><h2>Compiler and Debugger</h2><button disabled={busy || !ready} onClick={() => void perform(async () => setCompilation(await invoke<CompileState>('discover_toolchains')), 'Checking installed tools…')}>Scan / Refresh</button></div>
        <p>GCC: <code>{compilation.gcc?.path ?? 'Not selected'}</code></p>
        <p>GDB: <code>{compilation.gdb?.path ?? 'Not selected'}</code></p>
        <p className="hint">Select a validated GCC to compile. GDB is optional for compilation. Selections last until the app closes.</p>
        <form onSubmit={event => {
          event.preventDefault()
          void perform(async () => setCompilation(await invoke<CompileState>('validate_toolchain', { tool, path: toolPath })), 'Checking the selected tool…')
        }}>
          <label htmlFor="tool-path">Manual executable path</label>
          <div className="actions">
            <select aria-label="Tool" disabled={busy || !ready} value={tool} onChange={event => setTool(event.target.value as Tool)}><option>GCC</option><option>GDB</option></select>
            <input id="tool-path" disabled={busy || !ready} value={toolPath} onChange={event => setToolPath(event.target.value)} placeholder="Absolute path to gcc or gdb" />
            <button type="button" disabled={busy || !ready} onClick={() => void perform(async () => {
              const path = await open({ directory: false, multiple: false, title: `Choose ${tool}` })
              if (path) setToolPath(path)
            })}>Browse</button>
            <button disabled={busy || !ready || !toolPath.trim()}>Validate</button>
          </div>
        </form>
        <ul className="tools">{compilation.candidates.map(candidate => <li key={candidate.id}>
          <div><strong>{candidate.tool}</strong> · {candidate.usable ? 'Usable' : 'Unavailable'} · {candidate.origin}<br /><code>{candidate.path}</code><p>{candidate.version}</p>
            {candidate.error && <p className="tool-error">{candidate.error}</p>}
            {candidate.details && <details><summary>Validation details</summary><pre>{candidate.details}</pre></details>}
          </div>
          <button disabled={busy || !candidate.usable} onClick={() => void perform(async () => setCompilation(await invoke<CompileState>('select_toolchain', { id: candidate.id })))}>Select {candidate.tool}</button>
        </li>)}</ul>
      </section>
      <section className="compile-panel">
        <div className="section-title"><h2>Compile Sources</h2><span className="status">{compilation.build_valid && !dirty ? 'Current compilation available' : 'Compile required'}</span></div>
        <p className="hint">Select an Authored entry above, then add it here. Opening a file in the editor does not change these sources.</p>
        <div className="actions">
          <button disabled={busy || !selectedSource} onClick={() => void perform(() => setTargets({ ...targets, primary: selected!.relative_path, additional: targets.additional.filter(p => p !== selected!.relative_path) }))}>Set Primary .c</button>
          <button disabled={busy || !selectedSource || targets.primary === selected?.relative_path || targets.additional.includes(selected?.relative_path ?? '')} onClick={() => void perform(() => setTargets({ ...targets, additional: [...targets.additional, selected!.relative_path] }))}>Add Other .c</button>
          <button disabled={busy || !ready || !selected?.is_directory || targets.includes.includes(selected.relative_path)} onClick={() => void perform(() => setTargets({ ...targets, includes: [...targets.includes, selected!.relative_path] }))}>Add Include Folder</button>
          <button disabled={busy || !ready || targets.includes.includes(authored.path)} onClick={() => void perform(() => setTargets({ ...targets, includes: [...targets.includes, authored.path] }))}>Include Current Folder</button>
        </div>
        <ul className="source-list">
          <li><span>Primary: <code>{targets.primary ? displayPath(targets.primary) : 'Choose a .c file'}</code></span>{targets.primary && <button disabled={busy} onClick={() => void perform(() => setTargets({ ...targets, primary: null }))}>Remove</button>}</li>
          {targets.additional.map(path => <li key={`source:${path}`}><span>Other source: <code>{displayPath(path)}</code></span><button disabled={busy} onClick={() => void perform(() => setTargets({ ...targets, additional: targets.additional.filter(p => p !== path) }))}>Remove</button></li>)}
          {targets.includes.map(path => <li key={`include:${path}`}><span>Include: <code>Authored/{displayPath(path)}</code></span><button disabled={busy} onClick={() => void perform(() => setTargets({ ...targets, includes: targets.includes.filter(p => p !== path) }))}>Remove</button></li>)}
        </ul>
        <button className="primary" disabled={busy || !ready || !compilation.gcc || !targets.primary} onClick={() => void perform(async () => {
          setCompilation(previous => ({ ...previous, build_valid: false, output: null }))
          if (isDirty(currentDocument.current)) await save()
          setCompilation(await invoke<CompileState>('compile_sources'))
        }, 'Compiling…')}>{dirty ? 'Save & Compile' : 'Compile'}</button>
        <p className="hint">Compilation uses saved files and has a 60-second time limit. Editing and file operations pause while it runs.</p>
      </section>
      <section className="output-panel" aria-label="Compile Output">
        <h2>Compile Output</h2>
        {compilation.output ? <>
          <p className={compilation.output.success ? 'success' : 'error'} role="status">{compilation.output.success ? 'Compilation succeeded' : 'Compilation failed'} · Exit code: {compilation.output.exit_code ?? 'Not available'}</p>
          {compilation.output.error && <p className="tool-error">{compilation.output.error}</p>}
          <h3>Standard output</h3><pre>{compilation.output.stdout || '(No output)'}</pre>
          <h3>Warnings and errors</h3><pre>{compilation.output.stderr || '(No warnings or errors)'}</pre>
          {compilation.output.cleanup_errors.length > 0 && <div role="alert"><h3>Cleanup problems</h3><pre>{compilation.output.cleanup_errors.join('\n')}</pre></div>}
        </> : <p className="empty">Compile the selected sources to see results.</p>}
        {compilation.cleanup_errors.length > 0 && <div role="alert"><h3>Temporary files could not be removed</h3><pre>{compilation.cleanup_errors.join('\n')}</pre></div>}
        <p className="hint">Results describe the last compilation. Changed files require compilation again. External changes are checked when compiling or refreshing; indirect headers are not continuously monitored.</p>
      </section>
      <dialog ref={dialog} onCancel={event => { event.preventDefault(); choose('cancel') }} aria-labelledby="save-question">
        <h2 id="save-question">{question}</h2><p>Your unsaved changes are still in the editor.</p>
        <div className="actions"><button autoFocus onClick={() => choose('save')}>Save</button><button onClick={() => choose('discard')}>Discard</button><button onClick={() => choose('cancel')}>Cancel</button></div>
      </dialog>
    </main>
  )
}

createRoot(document.getElementById('root')!).render(<App />)

import { useEffect, useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'
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
type Listing = { path: string; entries: Entry[]; selected: string | null }
const emptyListing = (): Listing => ({ path: '', entries: [], selected: null })

function App() {
  const [settings, setSettings] = useState<Settings | null>(null)
  const [draft, setDraft] = useState<Preferences>({ workspace_root: null, temp_root: null })
  const [imported, setImported] = useState<Listing>(emptyListing)
  const [authored, setAuthored] = useState<Listing>(emptyListing)
  const [filename, setFilename] = useState('')
  const [busy, setBusy] = useState(true)
  const [error, setError] = useState('')
  const [message, setMessage] = useState('')
  const running = useRef(true)
  const ready = settings !== null && settings.error === null

  async function readListing(area: Area, path: string) {
    const setListing = area === 'Imported' ? setImported : setAuthored
    setListing({ path, entries: [], selected: null })
    const entries = await invoke<Entry[]>('list_workspace_entries', { area, relativePath: path })
    setListing({ path, entries, selected: null })
  }

  async function refresh(importedPath = imported.path, authoredPath = authored.path) {
    // The Rust boundary serializes access, including reads while switching roots.
    await readListing('Imported', importedPath)
    await readListing('Authored', authoredPath)
  }

  async function perform(action: () => Promise<void>) {
    if (running.current) return
    running.current = true
    setBusy(true)
    setError('')
    setMessage('')
    try { await action() }
    catch (cause) { setError(String(cause)) }
    finally { running.current = false; setBusy(false) }
  }

  useEffect(() => {
    async function load() {
      try {
        const current = await invoke<Settings>('get_workspace_settings')
        setSettings(current)
        setDraft(current.preferences)
        if (!current.error) await refresh('', '')
      } catch (cause) { setError(String(cause)) }
      finally { running.current = false; setBusy(false) }
    }
    void load()
  }, [])

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
          <code>{area}/{listing.path}</code>
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
              {entry.is_directory && <button disabled={busy || !ready} onClick={() => void perform(() => readListing(area, entry.relative_path))} aria-label={`Open ${entry.name}`}>Open</button>}
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

  return (
    <main aria-busy={busy}>
      <header><img className="mark" src={codelearnIcon} alt="" /><div><h1>CodeLearn Tool</h1><p>Your learning workspace, starting with C.</p></div></header>
      <section className="settings">
        <div className="section-title"><h2>Workspace Settings</h2><span className="status">{busy ? 'Loading / Busy' : error || !ready ? 'Error' : 'Ready'}</span></div>
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
            const current = await invoke<Settings>('apply_workspace_settings', { preferences: draft })
            setSettings(current)
            setDraft(current.preferences)
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
    </main>
  )
}

createRoot(document.getElementById('root')!).render(<App />)

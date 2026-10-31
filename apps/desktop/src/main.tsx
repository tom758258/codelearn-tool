import { useEffect, useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { invoke } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { open } from '@tauri-apps/plugin-dialog'
import CourseText from './CourseText'
import Workbench from './Workbench'
import SettingsPanel from './SettingsPanel'
import type { Breakpoint, CompileState, Course, DebugAction, DebugOutput, DebugStatus, Document, Library, OpenDocument, RunOutput, RunStatus, Settings, Targets, Unit, UnitKey } from './types'
import codelearnIcon from '../src-tauri/icons/codelearn-icon.png'
import './styles.css'

type Choice = 'save' | 'discard' | 'cancel' | 'confirm'
type Question = { kind: 'save' | 'cleanup' | 'reset'; text: string }
type Session = { id: string; kind: 'run' | 'debug'; done: Promise<void>; stopTask: Promise<void> | null; submitted: boolean; settled: boolean; stopRequested: boolean }
type Reply<T> = { Ok: T } | { Err: string }
const pause = (ms: number) => new Promise<void>(resolve => window.setTimeout(resolve, ms))
const emptyCompile = (): CompileState => ({ targets: { primary: null, additional: [], includes: [] }, candidates: [], gcc: null, gdb: null, build_valid: false, output: null, cleanup_errors: [], tool_errors: [] })
const dirty = (doc: OpenDocument) => doc.value !== doc.content

function App() {
  const [page, setPage] = useState<'Courses' | 'Projects' | 'Settings'>('Courses')
  const [settings, setSettings] = useState<Settings | null>(null)
  const [library, setLibrary] = useState<Library>({ courses: [], projects: [] })
  const [course, setCourse] = useState<{ key: string; data: Course; chapter: string; text: Record<string, string> } | null>(null)
  const [unit, setUnit] = useState<Unit | null>(null)
  const [documents, setDocuments] = useState<Record<string, OpenDocument>>({})
  const [activeFile, setActiveFile] = useState('')
  const [compilation, setCompilation] = useState<CompileState>(emptyCompile)
  const [busy, setBusy] = useState(true)
  const [closing, setClosing] = useState(false)
  const [activity, setActivity] = useState('Loading…')
  const [error, setError] = useState('')
  const [message, setMessage] = useState('')
  const [question, setQuestion] = useState<Question | null>(null)
  const [stdin, setStdin] = useState('')
  const [runOutput, setRunOutput] = useState<RunOutput | null>(null)
  const [debugView, setDebugView] = useState<DebugStatus | null>(null)
  const [sessionKind, setSessionKind] = useState<'run' | 'debug' | null>(null)
  const [stopping, setStopping] = useState(false)
  const [debugBusy, setDebugBusy] = useState(false)
  const [breakpoints, setBreakpoints] = useState<Breakpoint[]>([])
  const [cleanupWarnings, setCleanupWarnings] = useState<string[]>([])
  const docs = useRef<Record<string, OpenDocument>>({})
  const operating = useRef(true)
  const updates = useRef<Promise<void>>(Promise.resolve())
  const activeSession = useRef<Session | null>(null)
  const closePending = useRef(false)
  const allowClose = useRef(false)
  const pendingCleanup = useRef<string[]>([])
  const dialog = useRef<HTMLDialogElement>(null)
  const answer = useRef<((choice: Choice) => void) | null>(null)
  const closeAction = useRef<() => void>(() => {})
  const locked = busy || closing
  const ready = settings !== null && settings.error === null

  function changeDocs(value: Record<string, OpenDocument>) { docs.current = value; setDocuments(value) }
  function rememberCleanup(errors: string[]) {
    pendingCleanup.current = [...new Set([...pendingCleanup.current, ...errors])]
    setCleanupWarnings(pendingCleanup.current)
  }
  async function refreshCompile() {
    const view = await invoke<CompileState>('get_compile_state')
    setCompilation(view)
    rememberCleanup([...view.cleanup_errors, ...(view.output?.cleanup_errors ?? [])])
  }
  async function syncEditor() {
    await updates.current
    for (const doc of Object.values(docs.current)) await invoke('set_editor_dirty', { relativePath: doc.relative_path, dirty: dirty(doc) })
  }
  async function perform(action: () => Promise<void>, label = 'Working…') {
    if (operating.current) return
    operating.current = true; setBusy(true); setActivity(label); setError(''); setMessage('')
    try { await syncEditor(); await action() }
    catch (cause) { setError(String(cause)) }
    finally {
      try { await syncEditor(); await refreshCompile() } catch (cause) { setError(previous => previous || String(cause)) }
      operating.current = false; setBusy(false); setActivity('')
    }
  }
  useEffect(() => {
    void (async () => {
      try {
        const current = await invoke<Settings>('get_workspace_settings')
        setSettings(current)
        if (!current.error) setLibrary(await invoke<Library>('get_library'))
        setCompilation(await invoke<CompileState>('restore_toolchains'))
      } catch (cause) { setError(String(cause)) }
      finally { operating.current = false; setBusy(false); setActivity('') }
    })()
    let disposed = false
    const stop = getCurrentWindow().onCloseRequested(event => {
      if (allowClose.current) return
      event.preventDefault(); closeAction.current()
    })
    void stop.then(unlisten => { if (disposed) unlisten() }).catch(cause => setError(String(cause)))
    return () => { disposed = true; void stop.then(unlisten => unlisten()).catch(() => {}) }
  }, [])
  useEffect(() => { if (question && !dialog.current?.open) dialog.current?.showModal() }, [question])
  function ask(value: Question) {
    setQuestion(value)
    return new Promise<Choice>(resolve => { answer.current = resolve })
  }
  function choose(choice: Choice) {
    dialog.current?.close(); setQuestion(null)
    answer.current?.(choice); answer.current = null
  }
  async function saveAll() {
    for (const doc of Object.values(docs.current)) {
      if (!dirty(doc)) continue
      const saved = await invoke<Document>('save_authored_file', { relativePath: doc.relative_path, content: doc.value })
      changeDocs({ ...docs.current, [doc.relative_path]: { ...saved, value: saved.content, generation: doc.generation } })
    }
    setMessage('Changes saved.')
  }
  async function beforeSwitch(text: string): Promise<boolean | null> {
    if (!Object.values(docs.current).some(dirty)) return false
    const choice = await ask({ kind: 'save', text })
    if (choice === 'cancel') return null
    if (choice === 'save') await saveAll()
    return choice === 'discard'
  }
  function clearUnit(next: Unit | null) {
    setUnit(next); changeDocs({}); setActiveFile(''); setBreakpoints([]); setStdin(''); setRunOutput(null); setDebugView(null)
  }
  async function leaveUnit(reason: string) {
    const discardChanges = await beforeSwitch(reason)
    if (discardChanges === null) return false
    await invoke('open_unit', { key: null, discardChanges }); clearUnit(null)
    return true
  }
  function switchPage(next: typeof page) {
    if (next !== page) void perform(async () => { if (await leaveUnit('Save changes before leaving this exercise or project?')) setPage(next) })
  }
  async function loadDocument(path: string, reload = false) {
    if (!reload && docs.current[path]) return
    const opened = await invoke<Document>('read_authored_file', { relativePath: path, discardChanges: reload })
    changeDocs({ ...docs.current, [path]: { ...opened, value: opened.content, generation: (docs.current[path]?.generation ?? 0) + 1 } })
  }
  function openFile(path: string) {
    if (docs.current[path]) { setActiveFile(path); return }
    void perform(async () => { await loadDocument(path); setActiveFile(path) })
  }
  async function setTargets(targets: Targets) { setCompilation(await invoke<CompileState>('set_compile_targets', { targets })) }
  async function activate(key: UnitKey) {
    if (JSON.stringify(unit?.key) === JSON.stringify(key)) return
    const discardChanges = await beforeSwitch('Save changes before opening another exercise or project?')
    if (discardChanges === null) return
    const next = await invoke<Unit>('open_unit', { key, discardChanges })
    clearUnit(next)
    if (key.kind === 'project' && !next.targets.primary) {
      const sources = next.files.filter(file => file.name.endsWith('.c'))
      if (sources.length === 1) await setTargets({ primary: sources[0].relative_path, additional: [], includes: [] })
    }
    const first = next.targets.primary ?? next.files[0]?.relative_path
    if (first) { await loadDocument(first); setActiveFile(first) }
  }
  async function showChapter(key: string, data: Course, chapter: string) {
    const text: Record<string, string> = {}
    for (const block of data.chapters.find(item => item.id === chapter)!.blocks) if (block.kind === 'content') text[block.id] = await invoke<string>('get_course_content', { key, chapter, block: block.id })
    setCourse({ key, data, chapter, text })
  }
  function openCourse(key: string) {
    void perform(async () => {
      const data = await invoke<Course>('get_course', { key })
      if (await leaveUnit('Save changes before opening this course?')) await showChapter(key, data, data.chapters[0].id)
    })
  }
  function importFolder(isCourse: boolean) {
    void perform(async () => {
      const source = await open({ directory: true, multiple: false, title: isCourse ? 'Import Course Folder' : 'Import Project Folder' })
      if (!source) return
      setLibrary(await invoke<Library>('import_learning', { source, course: isCourse }))
      setMessage(isCourse ? 'Course imported. Choose it to start reading.' : 'Project imported. Choose it to edit your copy.')
    }, 'Importing folder…')
  }
  function edit(path: string, value: string) {
    const current = docs.current[path]
    const session = activeSession.current
    if (!current || closePending.current || (operating.current && session?.kind !== 'debug')) return
    const next = { ...current, value }
    changeDocs({ ...docs.current, [path]: next })
    if (dirty(next)) setCompilation(previous => ({ ...previous, build_valid: false }))
    if (session?.kind === 'debug') { void stopSession(); return }
    if (dirty(current) !== dirty(next)) updates.current = updates.current.then(() => invoke<void>('set_editor_dirty', { relativePath: path, dirty: dirty(next) })).catch(cause => { setError(String(cause)) })
  }
  async function recoverRun(cause: unknown): Promise<RunOutput> {
    for (;;) {
      let status: RunStatus
      try { status = await invoke<RunStatus>('get_run_state') }
      catch (error) { setError(`Cannot confirm program completion. Stop remains available. ${String(error)}`); await pause(250); continue }
      if (status.active) { await pause(100); continue }
      if (status.output) return status.output
      if (!status.error) rememberCleanup(['The run result could not be recovered. Process cleanup could not be confirmed.'])
      throw new Error(status.error ?? String(cause))
    }
  }
  async function recoverDebug(id: string, cause: unknown): Promise<DebugOutput> {
    for (;;) {
      let status: DebugStatus
      try { status = await invoke<DebugStatus>('get_debug_state') }
      catch (error) { setError(`Cannot confirm debugger completion. Stop remains available. ${String(error)}`); await pause(250); continue }
      if (status.id !== id) {
        rememberCleanup(['The debugger result could not be recovered. Cleanup could not be confirmed.'])
        throw new Error(String(cause))
      }
      setDebugView(status)
      if (!status.active) {
        if (status.output) return status.output
        throw new Error(status.error ?? String(cause))
      }
      await pause(100)
    }
  }
  async function pollDebug(session: Session) {
    while (!session.settled) {
      if (session.submitted) {
        try {
          const status = await invoke<DebugStatus>('get_debug_state')
          if (activeSession.current === session && status.id === session.id) setDebugView(status)
        } catch (cause) { setError(`Cannot read debugger state: ${String(cause)}`) }
      }
      await pause(120)
    }
  }
  function startSession(kind: Session['kind']) {
    if (operating.current || closePending.current || !unit) return
    const session: Session = { id: crypto.randomUUID(), kind, done: Promise.resolve(), stopTask: null, submitted: false, settled: false, stopRequested: false }
    activeSession.current = session; setSessionKind(kind); setStopping(false); setRunOutput(null); setDebugView(null)
    session.done = perform(async () => {
      try {
        await saveAll()
        if (kind === 'debug') for (const file of unit.files) await loadDocument(file.relative_path)
        if (session.stopRequested) return
        session.submitted = true
        if (kind === 'run') {
          let reply: Reply<RunOutput>
          try { reply = await invoke<Reply<RunOutput>>('run_program', { stdin }) }
          catch (cause) { reply = { Ok: await recoverRun(cause) } }
          if ('Err' in reply) throw new Error(reply.Err)
          const output = reply.Ok
          setRunOutput(output)
          rememberCleanup(output.cleanup_errors.map(error => output.working_directory ? `${error}\nTemporary directory: ${output.working_directory}` : error))
        } else {
          const polling = pollDebug(session)
          try {
            let reply: Reply<DebugOutput>
            try { reply = await invoke<Reply<DebugOutput>>('debug_program', { id: session.id, stdin, breakpoints }) }
            catch (cause) { reply = { Ok: await recoverDebug(session.id, cause) } }
            if ('Err' in reply) throw new Error(reply.Err)
            rememberCleanup(reply.Ok.cleanup_errors)
            const status = await invoke<DebugStatus>('get_debug_state')
            if (status.id === session.id) setDebugView(status)
          } finally { session.settled = true; await polling }
        }
      } finally { session.settled = true; await session.stopTask }
    }, kind === 'run' ? 'Saving, building if needed, and running…' : 'Saving, building if needed, and starting debugger…').finally(() => {
      session.settled = true; activeSession.current = null; setSessionKind(null); setStopping(false)
    })
  }
  function stopSession(): Promise<void> {
    const session = activeSession.current
    if (!session || session.settled) return Promise.resolve()
    session.stopRequested = true; setStopping(true); setActivity('Stopping and waiting for cleanup…')
    if (session.stopTask) return session.stopTask
    session.stopTask = (async () => {
      while (!session.settled && session.submitted) {
        try { if (await invoke<boolean>(session.kind === 'run' ? 'stop_run' : 'stop_debug', session.kind === 'debug' ? { id: session.id } : {})) return }
        catch (cause) { setError(`Could not request stop: ${String(cause)}`) }
        await pause(25)
      }
    })()
    return session.stopTask
  }
  async function debugAction(action: DebugAction) {
    const session = activeSession.current
    if (session?.kind !== 'debug' || session.stopRequested || debugBusy) return
    setDebugBusy(true); setError('')
    try {
      await invoke('debug_action', { id: session.id, action })
      const status = await invoke<DebugStatus>('get_debug_state')
      if (activeSession.current === session && status.id === session.id) setDebugView(status)
    } catch (cause) { setError(String(cause)); throw cause }
    finally { setDebugBusy(false) }
  }
  function toggleBreakpoint(file: string, line: number) {
    if (closing || stopping || debugBusy || (busy && sessionKind !== 'debug')) return
    const exists = breakpoints.some(point => point.file === file && point.line === line)
    void (async () => {
      if (activeSession.current?.kind === 'debug') {
        if (debugView?.state !== 'Paused') { setError('Pause the program before changing breakpoints.'); return }
        const point = debugView.breakpoints.find(point => point.file === file && point.line === line)
        await debugAction(point ? { kind: 'remove_breakpoint', id: point.id } : { kind: 'add_breakpoint', file, line })
      }
      setBreakpoints(points => exists ? points.filter(point => point.file !== file || point.line !== line) : [...points, { file, line }])
    })().catch(() => {})
  }
  closeAction.current = () => {
    if (closePending.current) return
    const session = activeSession.current
    if (operating.current && !session) { setError('Please wait for the current operation to finish before closing.'); return }
    closePending.current = true; setClosing(true)
    void (async () => {
      try {
        if (session) { void stopSession(); await session.done }
        let shouldClose = false
        await perform(async () => {
          const discardChanges = await beforeSwitch('Save all changes before closing?')
          if (discardChanges === null) return
          await invoke('open_unit', { key: null, discardChanges }); clearUnit(null)
          shouldClose = true
        })
        if (shouldClose && pendingCleanup.current.length && await ask({ kind: 'cleanup', text: 'Close with unresolved cleanup problems?' }) !== 'confirm') return
        if (shouldClose) { allowClose.current = true; await getCurrentWindow().close() }
      } catch (cause) { allowClose.current = false; setError(String(cause)) }
      finally { closePending.current = false; setClosing(false) }
    })()
  }
  function resetExercise() {
    void perform(async () => {
      if (await ask({ kind: 'reset', text: 'Restore this exercise from its original course version? Saved and unsaved changes to its supplied files will be replaced. Other exercises and extra files will be kept.' }) !== 'confirm') return
      const next = await invoke<Unit>('reset_exercise', { discardChanges: true })
      clearUnit(next)
      const first = next.targets.primary ?? next.files[0]?.relative_path
      if (first) { await loadDocument(first); setActiveFile(first) }
      setMessage('Exercise restored.')
    })
  }
  function editorPanel() {
    return unit && <Workbench unit={unit} documents={documents} activeFile={activeFile} compilation={compilation} locked={locked} debugEditable={!closing && sessionKind === 'debug' && !!activeSession.current?.submitted} sessionKind={sessionKind} stopping={stopping} debugBusy={debugBusy} breakpoints={breakpoints} runOutput={runOutput} debugView={debugView} stdin={stdin} onStdin={setStdin} onOpen={openFile} onEdit={edit} onBreakpoint={toggleBreakpoint} onTargets={targets => void perform(() => setTargets(targets))} onSave={() => void perform(saveAll)} onReload={() => void perform(async () => {
      if (docs.current[activeFile] && dirty(docs.current[activeFile])) {
        const choice = await ask({ kind: 'save', text: 'Save changes before reloading this file?' })
        if (choice === 'cancel') return
        if (choice === 'save') await saveAll()
      }
      await loadDocument(activeFile, true)
    })} onBuild={() => void perform(async () => { await saveAll(); setRunOutput(null); setDebugView(null); setCompilation(await invoke<CompileState>('compile_sources')) }, 'Saving and building…')} onRun={() => startSession('run')} onDebug={() => startSession('debug')} onStop={() => void stopSession()} onReset={resetExercise} onDebugAction={action => void debugAction(action).catch(() => {})} />
  }

  return <main>
    <header><img className="mark" src={codelearnIcon} alt="" /><div><h1>CodeLearn Tool</h1><p>Read, write, and explore C.</p></div></header>
    <nav className="main-nav" aria-label="Main navigation">{(['Courses', 'Projects', 'Settings'] as const).map(item => <button key={item} aria-current={page === item ? 'page' : undefined} disabled={locked} onClick={() => switchPage(item)}>{item}</button>)}</nav>
    <div role="status" className="activity">{activity || message}</div>
    {error && <div role="alert" className="error">{error}</div>}
    {settings?.error && <div role="alert" className="error">{settings.error} Open Settings to choose an available workspace.</div>}
    {compilation.tool_errors.length > 0 && <div role="alert" className="error">{compilation.tool_errors.map((error, i) => <p key={i}>{error}</p>)}<p>Review the selected tools in Settings.</p></div>}
    {cleanupWarnings.length > 0 && <section className="error" role="alert"><h3>Cleanup needs attention</h3>{cleanupWarnings.map((warning, i) => <pre key={i}>{warning}</pre>)}</section>}
    {page === 'Courses' && <div className="learning-layout"><aside className="library"><h2>Courses</h2><button disabled={locked || !ready} onClick={() => importFolder(true)}>Import course</button><ul>{library.courses.map(item => <li key={item.key}><button disabled={locked} aria-pressed={course?.key === item.key} onClick={() => openCourse(item.key)}>{item.title}<small>Version {item.version}</small></button></li>)}</ul>{!library.courses.length && <p className="hint">Import a complete course folder to start reading.</p>}{course && <><h3>Chapters</h3><ol>{course.data.chapters.map(chapter => <li key={chapter.id}><button disabled={locked} aria-pressed={course.chapter === chapter.id} onClick={() => void perform(async () => { if (await leaveUnit('Save changes before changing chapters?')) await showChapter(course.key, course.data, chapter.id) })}>{chapter.title}</button></li>)}</ol></>}</aside><article className="notebook">{course ? <><h2>{course.data.title}</h2><p className="hint">Version {course.data.version}</p>{course.data.chapters.find(chapter => chapter.id === course.chapter)?.blocks.map(block => <div key={block.id} className="notebook-block">{block.kind === 'content' ? <CourseText location={{ course: course.key, chapter: course.chapter, block: block.id }} text={course.text[block.id] ?? ''} /> : unit?.key.kind === 'course' && unit.key.course === course.key && unit.key.chapter === course.chapter && unit.key.block === block.id ? editorPanel() : <section className="exercise-card"><h3>{block.title}</h3><p>{block.files.join(' · ')}</p><button disabled={locked} onClick={() => void perform(() => activate({ kind: 'course', course: course.key, chapter: course.chapter, block: block.id }))}>Open exercise</button></section>}</div>)}</> : <section className="welcome"><h2>Learn one step at a time</h2><p>Choose a course to read its lessons and try the exercises. Your saved work stays separate from the original course.</p></section>}</article></div>}
    {page === 'Projects' && <div className="learning-layout"><aside className="library"><h2>Projects</h2><button disabled={locked || !ready} onClick={() => importFolder(false)}>Import project</button><ul>{library.projects.map(project => <li key={project.key}><button disabled={locked} aria-pressed={unit?.key.kind === 'project' && unit.key.project === project.key} onClick={() => void perform(() => activate({ kind: 'project', project: project.key }))}>{project.title}</button></li>)}</ul><p className="hint">Import a complete C project folder. Compile sources are remembered for each project.</p></aside><article>{unit?.key.kind === 'project' ? editorPanel() : <section className="welcome"><h2>Work on a C project</h2><p>Choose a project to edit, build, run, and debug your working copy.</p></section>}</article></div>}
    {page === 'Settings' && settings && <SettingsPanel settings={settings} compilation={compilation} locked={locked} ready={ready} perform={perform} onCompilation={setCompilation} onApply={async preferences => {
      const discardChanges = await beforeSwitch('Save changes before changing the workspace?')
      if (discardChanges === null) return
      const next = await invoke<Settings>('apply_workspace_settings', { preferences, discardChanges })
      setSettings(next); clearUnit(null); setCourse(null)
      setLibrary(await invoke<Library>('get_library')); setMessage('Workspace settings saved.')
    }} />}
    <dialog ref={dialog} onCancel={event => { event.preventDefault(); choose('cancel') }}><h2>{question?.kind === 'save' ? 'Unsaved changes' : question?.kind === 'reset' ? 'Reset exercise' : 'Cleanup warning'}</h2><p>{question?.text}</p><div className="actions">{question?.kind === 'save' ? <><button onClick={() => choose('cancel')}>Cancel</button><button onClick={() => choose('discard')}>Discard</button><button className="primary" onClick={() => choose('save')}>Save</button></> : <><button onClick={() => choose('cancel')}>{question?.kind === 'cleanup' ? 'Keep open' : 'Cancel'}</button><button onClick={() => choose('confirm')}>{question?.kind === 'cleanup' ? 'Close anyway' : 'Reset exercise'}</button></>}</div></dialog>
  </main>
}

createRoot(document.getElementById('root')!).render(<App />)

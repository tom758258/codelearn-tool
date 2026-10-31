import { useEffect, useState } from 'react'
import Editor from './Editor'
import type { Breakpoint, CompileState, DebugAction, DebugStatus, OpenDocument, RunOutput, Targets, Unit } from './types'

type Props = {
  unit: Unit; documents: Record<string, OpenDocument>; activeFile: string; compilation: CompileState
  locked: boolean; debugEditable: boolean; sessionKind: 'run' | 'debug' | null; stopping: boolean; debugBusy: boolean
  breakpoints: Breakpoint[]; runOutput: RunOutput | null; debugView: DebugStatus | null; stdin: string
  onStdin: (text: string) => void; onOpen: (path: string) => void; onEdit: (path: string, text: string) => void
  onBreakpoint: (file: string, line: number) => void; onTargets: (targets: Targets) => void
  onSave: () => void; onReload: () => void; onBuild: () => void; onRun: () => void; onDebug: () => void
  onStop: () => void; onReset: () => void; onDebugAction: (action: DebugAction) => void
}

export default function Workbench(props: Props) {
  const { unit, documents, activeFile, compilation, locked, debugEditable, sessionKind, stopping, debugBusy, breakpoints, runOutput, debugView } = props
  const [selectedFrame, setSelectedFrame] = useState(0)
  const hasDirty = Object.values(documents).some(doc => doc.value !== doc.content)
  const paused = debugView?.active && debugView.state === 'Paused' && !stopping
  const frame = debugView?.frames.find(frame => frame.level === selectedFrame) ?? debugView?.frames[0]
  const stoppedFile = paused ? debugView.frames[0]?.file : null
  const stoppedLine = paused ? debugView.frames[0]?.line : null
  const canBuild = !locked && !!compilation.gcc && !!compilation.targets.primary
  const targets = compilation.targets
  useEffect(() => {
    if (stoppedFile && documents[stoppedFile]) { props.onOpen(stoppedFile); setSelectedFrame(0) }
  }, [stoppedFile, stoppedLine])

  return <section className="editor-panel" aria-label={`Edit ${unit.title}`}>
    <div className="editor-toolbar"><h3>{unit.title}</h3><span className="status">{hasDirty ? 'Unsaved changes' : 'Saved'}</span></div>
    {unit.key.kind === 'project' && <details className="compile-settings" open={!targets.primary}>
      <summary>Compile sources</summary><fieldset disabled={locked}>
        <label htmlFor="primary-source">Main C file</label><select id="primary-source" value={targets.primary ?? ''} onChange={event => props.onTargets({ ...targets, primary: event.target.value || null, additional: targets.additional.filter(path => path !== event.target.value) })}>
          <option value="">Choose a main C file</option>{unit.files.filter(file => file.name.endsWith('.c')).map(file => <option key={file.relative_path} value={file.relative_path}>{file.name}</option>)}
        </select><p>Additional C files</p>
        {unit.files.filter(file => file.name.endsWith('.c') && file.relative_path !== targets.primary).map(file => <label className="check" key={file.relative_path}><input type="checkbox" checked={targets.additional.includes(file.relative_path)} onChange={event => props.onTargets({ ...targets, additional: event.target.checked ? [...targets.additional, file.relative_path] : targets.additional.filter(path => path !== file.relative_path) })} />{file.name}</label>)}
        <p>Include folders</p>{unit.directories.map(folder => <label className="check" key={folder.relative_path}><input type="checkbox" checked={targets.includes.includes(folder.relative_path)} onChange={event => props.onTargets({ ...targets, includes: event.target.checked ? [...targets.includes, folder.relative_path] : targets.includes.filter(path => path !== folder.relative_path) })} />{folder.name}</label>)}
      </fieldset></details>}
    <div className="file-tabs" role="tablist" aria-label="Source files">{unit.files.map(file => <button key={file.relative_path} role="tab" aria-selected={activeFile === file.relative_path} disabled={locked && !documents[file.relative_path]} onClick={() => props.onOpen(file.relative_path)}>{file.name}{documents[file.relative_path] && documents[file.relative_path].value !== documents[file.relative_path].content ? ' *' : ''}</button>)}</div>
    {Object.values(documents).map(doc => <div key={`${doc.relative_path}:${doc.generation}`} hidden={activeFile !== doc.relative_path}><Editor initialValue={doc.value} filename={unit.files.find(file => file.relative_path === doc.relative_path)?.name ?? doc.display_path} readOnly={locked && !debugEditable} onChange={value => props.onEdit(doc.relative_path, value)} breakpoints={breakpoints.filter(point => point.file === doc.relative_path).map(point => point.line)} currentLine={paused && frame?.file === doc.relative_path ? frame.line ?? undefined : undefined} onBreakpoint={line => props.onBreakpoint(doc.relative_path, line)} /></div>)}
    {!activeFile && <p className="empty">Choose a source file to edit.</p>}
    <p className="hint">Click beside a line number to add or remove a breakpoint. Editing during debugging stops the program and keeps your changes.</p>
    <div className="actions">
      <button disabled={locked || !hasDirty} onClick={props.onSave}>Save</button>
      <button disabled={locked || !activeFile} onClick={props.onReload}>Reload file</button>
      <button disabled={!canBuild} onClick={props.onBuild}>Build</button>
      <button className="primary" disabled={!canBuild} onClick={props.onRun}>Run</button>
      <button disabled={!canBuild || !compilation.gdb} onClick={props.onDebug}>Debug</button>
      {sessionKind && <button className="stop" disabled={stopping} onClick={props.onStop}>{stopping ? 'Stopping…' : 'Stop'}</button>}
      {unit.key.kind === 'course' && <button disabled={locked} onClick={props.onReset}>Reset exercise</button>}
    </div>
    {!compilation.gcc && <p className="hint">Choose GCC in Settings to build and run. Reading and editing are available without it.</p>}
    {!!compilation.gcc && !compilation.gdb && <p className="hint">Choose GDB in Settings to debug.</p>}
    <details className="input-panel"><summary>Standard input</summary><label htmlFor="stdin">Text to send when the program starts</label><textarea id="stdin" rows={3} value={props.stdin} disabled={locked} onChange={event => props.onStdin(event.target.value)} /><p className="hint">Input closes after this text is sent. Running programs have a 30 second time limit.</p></details>
    {compilation.output && <section className="output-panel" aria-label="Build output"><h3>Build {compilation.output.success ? 'succeeded' : 'failed'}</h3>{compilation.output.error && <p className="tool-error">{compilation.output.error}</p>}{compilation.output.stdout && <pre>{compilation.output.stdout}</pre>}{compilation.output.stderr && <pre>{compilation.output.stderr}</pre>}{compilation.output.cleanup_errors.map((error, i) => <p key={i} className="tool-error">{error}</p>)}</section>}
    {runOutput && <section className="output-panel" aria-label="Program output"><h3>Program output</h3><p>{runOutput.outcome?.replace(/([a-z])([A-Z])/g, '$1 $2') ?? 'Could not start'}{runOutput.exit_code !== null ? ` · Exit code ${runOutput.exit_code}` : ''}</p>{runOutput.start_error && <p className="tool-error">{runOutput.start_error}</p>}<pre>{runOutput.stdout || '(No standard output)'}</pre>{runOutput.stderr && <><h4>Standard error</h4><pre>{runOutput.stderr}</pre></>}{runOutput.process_errors.map((error, i) => <p className="tool-error" key={i}>{error}</p>)}</section>}
    {debugView && <section className="debug-panel" aria-label="Debugger"><div className="editor-toolbar"><h3>Debugger</h3><span>{debugView.state}</span></div>
      <div className="actions">{(['continue', 'step_into', 'step_over', 'step_out'] as const).map((kind, i) => <button key={kind} disabled={!paused || debugBusy} onClick={() => props.onDebugAction({ kind })}>{['Continue', 'Step into', 'Step over', 'Step out'][i]}</button>)}<button disabled={!debugView.active || debugView.state !== 'Running' || stopping || debugBusy} onClick={() => props.onDebugAction({ kind: 'pause' })}>Pause</button></div>
      <div className="debug-details"><section><h4>Variables</h4>{paused ? <table><thead><tr><th>Name</th><th>Value</th></tr></thead><tbody>{debugView.variables.map((variable, index) => <tr key={index}><td>{variable.name}{variable.is_argument ? ' (argument)' : ''}</td><td>{variable.value ?? 'Unavailable'}</td></tr>)}</tbody></table> : <p className="hint">Variables are available while paused.</p>}</section><section><h4>Call stack</h4><ul className="stack">{debugView.frames.map(frame => <li key={frame.level}><button disabled={!paused || debugBusy} aria-pressed={selectedFrame === frame.level} onClick={() => { setSelectedFrame(frame.level); if (frame.file && documents[frame.file]) props.onOpen(frame.file); props.onDebugAction({ kind: 'variables', frame: frame.level }) }}>{frame.function ?? 'Unknown function'} · {frame.display_file ?? 'Unknown source'}{frame.line ? `:${frame.line}` : ''}</button></li>)}</ul></section></div>
      <h4>Program output</h4><pre>{debugView.stdout || '(No standard output)'}</pre>{debugView.stderr && <><h4>Standard error</h4><pre>{debugView.stderr}</pre></>}
      {debugView.output && <><p>{debugView.output.outcome}{debugView.output.exit_code !== null ? ` · Exit code ${debugView.output.exit_code}` : ''}</p>{debugView.output.process_errors.map((error, i) => <p className="tool-error" key={i}>{error}</p>)}</>}
      {debugView.error && <p className="tool-error">{debugView.error}</p>}<details><summary>Debugger messages</summary><pre>{debugView.events.join('\n')}</pre></details>
    </section>}
  </section>
}

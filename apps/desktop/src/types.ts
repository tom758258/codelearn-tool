export type Preferences = { workspace_root: string | null; temp_root: string | null }
export type Settings = {
  preferences: Preferences
  default_workspace_root: string
  default_temp_root: string
  resolved_workspace_root: string | null
  resolved_temp_root: string | null
  error: string | null
}
export type Targets = { primary: string | null; additional: string[]; includes: string[] }
export type Tool = 'GCC' | 'GDB'
export type CompileState = {
  targets: Targets
  candidates: { id: number; tool: Tool; path: string; origin: string; version: string | null; usable: boolean; error: string | null; details: string }[]
  gcc: { path: string; version: string | null } | null
  gdb: { path: string; version: string | null } | null
  build_valid: boolean
  output: { success: boolean; exit_code: number | null; stdout: string; stderr: string; error: string | null; cleanup_errors: string[] } | null
  cleanup_errors: string[]
  tool_errors: string[]
}
export type Library = {
  courses: { key: string; id: string; version: string; title: string }[]
  projects: { key: string; title: string; targets: Targets }[]
}
export type Block = { kind: 'content'; id: string; markdown: string } | { kind: 'program'; id: string; title: string; directory: string; files: string[]; targets: Targets }
export type Course = { id: string; version: string; title: string; chapters: { id: string; title: string; blocks: Block[] }[] }
export type UnitKey = { kind: 'course'; course: string; chapter: string; block: string } | { kind: 'project'; project: string }
export type Unit = {
  key: UnitKey; title: string; root: string
  files: { relative_path: string; name: string }[]
  directories: { relative_path: string; name: string }[]
  targets: Targets
}
export type Document = { relative_path: string; display_path: string; content: string }
export type OpenDocument = Document & { value: string; generation: number }
export type RunOutput = {
  outcome: string | null; exit_code: number | null; stdout: string; stderr: string
  start_error: string | null; process_errors: string[]; cleanup_errors: string[]; working_directory: string | null
}
export type RunStatus = { active: boolean; output: RunOutput | null; error: string | null }
export type DebugOutput = { outcome: string; exit_code: number | null; process_errors: string[]; cleanup_errors: string[] }
export type Breakpoint = { file: string; line: number }
export type DebugStatus = {
  id: string; unit: UnitKey | null; active: boolean; stopping: boolean; state: string
  frames: { level: number; function: string | null; file: string | null; display_file: string | null; line: number | null }[]
  variables: { name: string; value: string | null; is_argument: boolean }[]
  breakpoints: (Breakpoint & { id: string })[]
  stdout: string; stderr: string; events: string[]; output: DebugOutput | null; error: string | null
}
export type DebugAction = { kind: 'continue' | 'step_into' | 'step_over' | 'step_out' | 'pause' } |
  { kind: 'add_breakpoint'; file: string; line: number } | { kind: 'remove_breakpoint'; id: string } | { kind: 'variables'; frame: number }

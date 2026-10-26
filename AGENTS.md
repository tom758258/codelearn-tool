# Agent Instructions

These instructions define long-term, repository-specific boundaries for agents working on `codelearn-tool`.
Keep changes small, preserve clear component ownership, and do not build future layers before a concrete requirement exists.

## 1. Project Context

- Read the affected code and relevant documentation before changing behavior.
- CodeLearn Tool is a Windows and Linux desktop application for learning programming, initially C. It is not intended to replace a full IDE.
- The intended product structure is independent Rust Core + Tauri 2 Desktop (React, TypeScript, Vite). Do not add CLI or Worker layers without explicit approval.
- Treat `project-plan.md`, when present, as planning context. Future phases are not authorization to implement them.

## 2. Repository And Architecture Boundaries

The intended initial repository structure is:

```text
codelearn-tool/
├─ crates/
│  └─ codelearn-tool-core/
├─ apps/
│  └─ desktop/
│     └─ src-tauri/
├─ Cargo.toml
├─ Cargo.lock
├─ README.md
└─ AGENTS.md
```

Architecture rules:

- The root Cargo workspace owns `codelearn-tool-core` and excludes `apps/desktop/src-tauri`.
- Desktop/Tauri is a separate Cargo package and lockfile; it depends on Core via a local path dependency.
- Core owns reusable toolchain, build, run, debug, validation, and learning logic **only when the corresponding feature is approved**.
- Core must not depend on Tauri, React, TypeScript, WebView APIs, or other presentation code.
- Keep Tauri commands thin. Desktop may depend on Core, but Core must not depend on Desktop.
- Do not create empty crates, a CLI, Worker, server, or generic plugin framework for possible future use.

## 3. Compiler And Debugger Boundary

- GCC/GDB are external toolchains, not part of the Rust Core or bundled Desktop by default.
- When these features are authorized, keep toolchain discovery, version checks, compilation, process execution, and debug lifecycle logic in Core.
- Do not assume that a command found on `PATH` is compatible without appropriate validation.
- Prefer existing working toolchains; do not silently install, replace, or update them, or change the system `PATH`.
- Toolchain installation requires explicit user approval. Do not assume GDB/MI versus DAP is settled before technical validation.
- Initial C support must not trigger premature C++ or multi-language abstraction work.

## 4. Desktop And Learning Boundary

- React owns presentation and user interaction; do not duplicate Core decisions in frontend code.
- Keep the intended learning flow focused on lessons, C editing, compile/run, and interactive debugging when the relevant phases are approved.
- Do not introduce course engines, progress storage, Monaco integrations, or debugging UI during Phase 0 simply because they appear in the roadmap.
- Do not reuse the orchestrator's Worker/Common contracts, workflow engine, or test-record management features without a concrete requirement.

## 5. Student Program Execution And Safety

- Treat student-authored source code and compiled programs as untrusted local inputs.
- When execution features are implemented, design explicit start/stop, timeouts, cancellation, process cleanup, and error reporting, including failed compilation and debugger termination paths.
- Do not claim that ordinary process supervision or AppImage packaging provides a security sandbox. Any stronger isolation requires separate design and validation.
- Never automatically execute, upload, or share student programs outside an explicitly authorized workflow.

## 6. Data, Logging, And Privacy

- Treat student files, unsaved code, build output, console text, debugger variables, traces, crash dumps, local toolchain paths, and learning records as private by default.
- Do not commit user/student workspaces, local configuration, personal absolute paths, secrets, private network addresses, raw runtime logs, or real debugging evidence to a public repository.
- Use synthetic examples and narrowly scoped tracked fixtures only when necessary for meaningful tests.
- Do not add telemetry, automatic cloud sync, account services, or remote code uploads without explicit approval.
- Keep persistence lightweight; do not introduce SQL databases, run-history systems, or student-record platforms without an approved requirement.
- `.gitignore` is not a privacy review. Inspect staged changes for sensitive files before any authorized commit.

## 7. Dependencies And Packaging

- Add dependencies only when required by the current implementation. Prefer established libraries and avoid premature async runtimes or frameworks.
- Keep Rust and npm lockfiles tracked; use npm with Node.js 24 and `npm ci` in CI for reproducibility.
- Windows and Linux are first-class targets. Avoid unnecessary platform-specific assumptions in Core.
- Automatic CI checks Frontend once and Core/Tauri on both operating systems; triggers are `main` pushes and pull requests.
- Keep Desktop release builds in one manual `build-desktop.yml` workflow with Windows / Linux / Both selection.
- Windows produces a standalone EXE; Linux produces an AppImage built on Ubuntu 24.04 x64. Do not introduce installers or `.deb` packages without approval.
- Upload platform-specific artifacts labeled with a short commit SHA and retain them for 30 days. Do not confuse build success with verified desktop runtime compatibility.

## 8. Testing And Validation

- Test the behavior actually changed. Prioritize important contracts, cross-component boundaries, failure handling, privacy, and execution safety.
- Do not automatically add a permanent regression test for every minor fix. Prefer focused tests over redundant or implementation-coupled coverage.
- Keep default Phase 0 tests independent of installed GCC/GDB and student programs. Introduce real toolchain validation only when the relevant feature is implemented.
- Run the narrowest relevant checks first, then broader checks when practical; do not create scaffolding merely to make a test command exist.

Core baseline checks (from the repository root):

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --locked
```

Desktop baseline checks (when the Desktop exists):

```text
cd apps/desktop && npm ci && npm run typecheck && npm run build
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml --check
cargo clippy --locked --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --locked --manifest-path apps/desktop/src-tauri/Cargo.toml
```

- Run frontend tests when applicable and present. Validate Windows/Linux jobs on their respective runners.
- Report actual results as Passed, Failed, Skipped, Blocked, or Not Run; never describe an unexecuted check as passing.

## 9. Scope Control

- Prefer the smallest implementation that satisfies the approved requirement. Do not implement future phases opportunistically.
- Avoid unrelated refactors and speculative generic abstractions.
- Ask for explicit approval before changing architecture ownership, toolchain installation behavior, student-data privacy or persistence, process-execution safety, public application identity, or distribution strategy.
- Do not commit, push, create releases, or change repository settings unless explicitly authorized.

## 10. Documentation Boundary

- Keep the root `README.md` as the project overview, setup guide, and primary entry point, not a complete specification dump.
- Keep durable architecture or contract detail in focused documentation only when the implemented feature requires it.
- Separate user-facing instructions from maintainer/build notes when the project grows.
- Maintain user-facing documentation in both English and Traditional Chinese (`zh-TW`). Keep their content semantically aligned and update both versions in the same change when the documented behavior or guidance changes.
- Do not create empty translated guides, duplicated internal documentation, or a Help generator solely to satisfy the bilingual documentation rule.
- Do not put temporary validation reports, local evidence, personal context, or private paths in tracked public documentation.
- Update documentation when implemented behavior, supported platforms, installation steps, or build requirements change.

## 11. Change Discipline

Before completing a change:

1. Confirm the implementation remains within the authorized scope.
2. Confirm Core/Desktop ownership and Windows/Linux boundaries remain clear.
3. Confirm no unnecessary dependency, data collection, or future-layer scaffolding was added.
4. Check that no student data or private validation artifacts are staged for publication.
5. Run the relevant checks and accurately report what passed, failed, was skipped, blocked, or not run.
6. Summarize the changes and explicitly identify remaining validation gaps.

When a smaller design fully satisfies the requirement, prefer it over a more elaborate design.

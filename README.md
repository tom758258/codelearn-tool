# CodeLearn Tool

A Windows and Linux desktop application for learning programming, starting with C. It is intended for focused learning, rather than replacing a full IDE.

面向 Windows 與 Linux 的程式學習桌面應用程式，從 C 語言開始，以學習為主，不以取代完整 IDE 為目標。

## Current scope / 目前範圍

The Tauri 2 desktop provides **Courses**, **Projects**, and **Settings**. Read ordered Markdown lessons and local images, edit independent exercise copies in multiple Monaco tabs, and build, run, or debug C programs. Import complete course folders or ordinary C project folders. GCC/GDB selections persist and are validated again at startup.

Tauri 2 桌面程式提供 **Courses**、**Projects** 與 **Settings**。可依序閱讀 Markdown 教材與本機圖片，在多個 Monaco 分頁編輯獨立的練習副本，並建置、執行或除錯 C 程式。支援完整課程資料夾及一般 C 專案資料夾匯入；GCC/GDB 選擇會保存，並於啟動時重新驗證。

Toolchain installation, course authoring, and learning progress are **not implemented**. Files and settings stay on this machine. No telemetry, cloud services, or learning-record storage is included. See the bilingual [course format guide](docs/course-format.md) and [sample course](examples/first-steps).

工具鏈安裝、課程編製及學習進度均**尚未實作**。檔案與設定保留在本機，不包含遙測、雲端服務或學習紀錄儲存。請參閱雙語的[課程格式說明](docs/course-format.md)與[範例課程](examples/first-steps)。

Core callers explicitly select an absolute GCC executable path and provide existing, separate Workspace and CodeLearnTemp directories. Selected `.c` files, logical source directories, and include directories must resolve inside the Workspace. Text input is compiled from a private snapshot and may link additional selected `.c` files. GCC is validated by a real compile/link; GDB is checked independently for version/startup only. Discovery suggests usable candidates in explicit-path, PATH, then common-location order and never selects one automatically.

Core 呼叫端須明確選定 GCC 執行檔的絕對路徑，並提供已存在、互不包含的 Workspace 與 CodeLearnTemp 目錄。選定的 `.c`、邏輯來源目錄與 Include 目錄，解析後必須位於 Workspace 內。文字來源會建立私有快照，亦可連結其他明確選定的 `.c`。GCC 以真實編譯／連結驗證；GDB 僅獨立檢查版本與可啟動性。候選建議順序先列可用工具，再依手動路徑、PATH、常見位置排序，不自動選定。

Each build uses C11 with `-Wall -Wextra -g -O0 -fdiagnostics-color=never`, preserves raw output and source paths, and has a configurable timeout (60 seconds by default). Only success returns a `BuildArtifact`; clones retain its executable and snapshot until the final drop. Keep a clone of `BuildResult.cleanup` to inspect directory cleanup errors after release. Core deletes only its own build subdirectory. Crashes may leave files; no startup scavenging or build cache is provided.

每次建置使用 C11 與 `-Wall -Wextra -g -O0 -fdiagnostics-color=never`，保留原始輸出與來源路徑，並提供可配置的逾時（預設 60 秒）。成功時才回傳 `BuildArtifact`；複製的引用會保留執行檔與快照，直到最後一份釋放。若要在釋放後檢查目錄清理錯誤，須保留 `BuildResult.cleanup` 的複本。Core 只刪除自己建立的建置子目錄。異常崩潰可能留下檔案；不提供啟動掃描清理或建置快取。

Compiler supervision uses Windows Job Objects or Linux process groups, including descendant cleanup on timeout and normal compiler exit. Output is captured in private files to avoid pipe deadlocks. GCC inherits the process environment and manages its own intermediate-file locations. Supervision and canonical path checks are not an OS sandbox and do not contain processes that deliberately escape their managed group, or all indirect header/library access.

編譯程序透過 Windows Job Object 或 Linux Process Group 管理，在逾時與編譯器正常退出時清理受控衍生程序。輸出擷取至私有檔案以避免管線死鎖。GCC 繼承程序環境，並自行管理中間檔位置。程序管理與實際路徑核對不是作業系統沙箱，無法涵蓋刻意逃離管理群組的程序或所有間接 Header／Library 存取。

Core callers can run a `BuildArtifact` with `run::start(RunRequest::new(artifact, workspace_root))`, request cancellation through `session.stop()` or a shareable `stop_handle()`, and collect the result with `session.wait()`. Dropping a session requests stop without waiting for cleanup. Stdin is pre-supplied bytes and closes after writing; stdout/stderr are drained concurrently as raw bytes and returned after cleanup. Timeout defaults to 30 seconds and accepts any positive `Duration`. Output is limited to a fixed **25 MiB combined** across stdout/stderr; exceeding it terminates the controlled processes and retains only the allowed bytes. Nonzero exit codes remain normal process exits. Start errors, execution errors, and process cleanup errors are reported separately.

Core 呼叫端可透過 `run::start(RunRequest::new(artifact, workspace_root))` 執行 `BuildArtifact`，使用 `session.stop()` 或可共享的 `stop_handle()` 要求停止，再以 `session.wait()` 取得結果。釋放 Session 會要求停止，但不會同步等待清理。stdin 預先提供原始 Bytes，寫完即關閉；stdout/stderr 同時排空，完成清理後統一回傳原始 Bytes。逾時預設 30 秒，可指定任意正值 `Duration`。stdout/stderr 的**合計上限固定為 25 MiB**；實際超限才終止受控程序，並保留額度內資料。非零 Exit Code 仍屬一般程序退出。啟動、執行與程序清理錯誤分開回報。

Each run uses a fresh working directory within its artifact. Optionally select an `input_root` inside the Workspace and explicit relative `input_files`; only those files are copied, preserving their directory structure. Changes to the copies do not overwrite the source files. Successful runs keep their directories until the final artifact reference is released; `RunResult.artifact` retains them while the result is alive. Files in these directories are temporary. Failed preparation/startup removes only the partial run directory where possible and reports cleanup failures.

每次執行均在 Artifact 內建立新的工作目錄。需要輸入檔時，指定 Workspace 內的 `input_root` 與明確列出的相對 `input_files`；僅複製這些檔案並保留目錄結構。修改副本不會覆寫來源檔。成功啟動的執行目錄保留至最後一份 Artifact 引用釋放；`RunResult.artifact` 會在結果存活期間保留它們。這些檔案是暫存資料。準備或啟動失敗時會盡可能只刪除當次部分目錄，並回報清理失敗。

If process cleanup fails, Core retries once using the existing finite waits. Recovery also removes a failed startup's partial run directory. If cleanup still cannot be confirmed, the supervisor stops retrying and releases its process handles; the entire artifact directory, including its other run directories, remains on disk after the final reference is released. The start error or run result and `BuildResult.cleanup` report the failures, retained path, and manual cleanup requirement. Confirm that all processes using that artifact have terminated before deleting it. There is no later automatic cleanup; OS failures may leave processes running or unreaped.

程序清理失敗時，Core 會沿用既有的有限等待再試一次；重試成功也會刪除啟動失敗所建立的部分 Run 目錄。若仍無法確認清理完成，監督執行緒會停止重試並釋放持有的程序控制代碼；整個 Artifact 目錄（包含其下其他 Run 目錄）會在最後引用釋放後繼續保留於磁碟。啟動錯誤或執行結果，以及 `BuildResult.cleanup`，都會回報失敗原因、保留路徑與手動清理要求。刪除前必須確認所有使用該 Artifact 的程序均已終止。之後不會自動清理；OS 故障仍可能留下執行中或未回收的程序。

Runs reuse Windows Job Objects and Linux process groups, cleaning up controlled descendants after normal exit, stop, timeout, or output overflow. Pipe collection has a bounded cleanup period and reports pipes that remain open. If the OS refuses I/O cancellation, an existing pipe worker may remain blocked while owning its pipe and an artifact reference; this is reported as a cleanup failure. This is **not a sandbox**: student programs retain ordinary OS access, can access other files with absolute paths or `..`, and may escape process-group control. The output cap does not limit program memory, generated files, or total memory across multiple sessions.

執行流程沿用 Windows Job Object 與 Linux Process Group，在正常退出、停止、逾時或輸出超限後清理受控後代。管線收集有清理等待期限，未關閉的管線會回報錯誤。若 OS 拒絕取消 I/O，既有管線執行緒仍可能阻塞並持有管線與 Artifact 引用；此情況會回報為清理失敗。這**不是沙箱**：學生程式仍有一般作業系統權限，可透過絕對路徑或 `..` 存取其他檔案，也可能逃離程序群組控制。輸出上限不限制程式記憶體、產生的檔案或多個 Session 的總記憶體。

Core callers can debug a `BuildArtifact` with `debug::start(DebugRequest::new(artifact, gdb, workspace_root))`, explicitly selecting an absolute GDB path. Startup disables GDB init files and auto-loading, checks MI3 capabilities, and requests execution to `main`. Consume `session.take_events()` while using breakpoints, continue/step, pause, call stack, and frame variables. Command acceptance and a later `Stopped` event are separate; only the actual stop confirms main entry or completion of a step/pause. One local inferior uses all-stop mode; restart, attach, remote debugging, watch expressions, and interactive stdin are not provided.

Core 呼叫端可使用 `debug::start(DebugRequest::new(artifact, gdb, workspace_root))` 除錯 `BuildArtifact`，並明確選定 GDB 絕對路徑。啟動時停用 GDB 初始化檔與自動載入、確認 MI3 能力，再要求執行至 `main`。操作斷點、繼續／步進、暫停、呼叫堆疊與指定 Frame 變數時，須持續讀取 `session.take_events()`。命令接受與稍後的 `Stopped` 事件分開；只有實際停止才能確認進入 main 或完成步進／暫停。首版僅支援單一本機 Inferior 與 All-stop，不提供重新啟動、Attach、遠端除錯、Watch Expression 或互動式 stdin。

Debugging uses the same selected runtime-file boundaries as Run, a separate working directory, pre-supplied stdin, and incremental raw stdout/stderr events, with a fixed **25 MiB combined** limit. Result data retains the artifact and errors without duplicating output. A slow/disconnected event consumer or oversized MI reply ends the session with an observable error. Startup, command, and running timeouts default to 15, 10, and 30 seconds, accept positive durations, and cannot be disabled; running time resets on each transition to Running and excludes stopped time. Stop and Drop request asynchronous cleanup.

除錯沿用 Run 的指定輸入檔邊界，使用獨立工作目錄、預先 stdin 與增量原始 stdout/stderr 事件，**合計上限固定為 25 MiB**。結果保留 Artifact 與錯誤，不重複保存完整輸出。事件接收端過慢／中斷或 MI 回覆過長，會以可觀察的錯誤結束 Session。啟動、命令與 Running 逾時預設分別為 15、10、30 秒，接受正值 Duration，不能停用；每次進入 Running 重新計時，不計入停止等待時間。Stop 與 Drop 會要求非同步清理。

On Windows, Core uses an existing short-name path for the same debug working directory when available. If Windows provides no usable short name, GDB can still fail to launch from paths its code page cannot represent.

Windows 上，Core 會在可用時採用同一除錯工作目錄既有的短名稱路徑。若 Windows 未提供可用的短名稱，GDB 仍可能無法從其 Code Page 無法表示的路徑啟動程序。

Debug cleanup uses the existing Windows Job Object; Linux uses an exclusive session and pidfds because GDB creates separate inferior process groups. Linux requires `/proc` and pidfd support (available on the Ubuntu 24.04 target). The host init/reaper must collect orphaned exited processes; Core does not change the application's global subreaper setting. If processes remain after finite cleanup, including zombies awaiting host reaping, cleanup errors are reported and the artifact is retained. This is process supervision, not a sandbox.

除錯清理沿用 Windows Job Object；Linux 因 GDB 會另建 Inferior 程序群組，改用獨立 Session 與 pidfd，需支援 `/proc` 與 pidfd（Ubuntu 24.04 目標具備）。孤兒程序退出後由宿主 Init／Reaper 回收；Core 不改動應用程式的全域 Subreaper 設定。有限清理後若仍有殘留，包含等待宿主回收的 Zombie，會回報清理錯誤並保留 Artifact。此機制是程序管理，不是沙箱。

## Courses and projects / 課程與專案

In **Courses**, choose **Import course** and select a complete folder containing `course.json`. Choose a course and chapter to read its text and images in order; **Open exercise** creates a working copy only when needed. Reading does not require GCC/GDB. Markdown HTML and remote images are not executed or loaded. Code snippets in lesson text are read-only.

在 **Courses** 選擇 **Import course**，匯入包含 `course.json` 的完整資料夾。選擇課程與章節，即可依序閱讀文字與圖片；按 **Open exercise** 才會在需要時建立練習副本。閱讀不需要 GCC/GDB；Markdown 中的 HTML 與遠端圖片不會執行或載入，教材內的程式片段僅供閱讀。

The same course ID and version cannot be imported twice. Different versions and exercises keep separate student files. **Reset exercise** asks before restoring only that exercise's supplied files from the same original version. It keeps extra files and other exercises. Reset is unavailable until an active program or debugger finishes cleanup. See the format guide for file and path requirements and partial-failure behavior.

相同課程 ID 與版本不可重複匯入；不同版本及練習各自保存學生檔案。**Reset exercise** 會先確認，再從同版本原始教材恢復該練習提供的檔案，保留額外檔案與其他練習。活動程式或除錯程序尚未完成清理時不能重設。檔案、路徑要求及部分失敗的處理方式請見格式說明。

In **Projects**, choose **Import project** to copy an ordinary C project folder. No CodeLearn configuration file is required in the source. Under **Compile sources**, select the main `.c`, additional `.c` files, and include folders. A project with one C file selects it automatically. Choices are saved in workspace management data, outside the copied source folder. Makefile/CMake and multiple build targets are not supported.

在 **Projects** 選擇 **Import project**，複製一般 C 專案資料夾；來源不需要 CodeLearn 設定檔。在 **Compile sources** 選定主要 `.c`、其他 `.c` 與 Include 資料夾；只有一個 C 檔案的專案會自動選定。選擇保存於工作區管理資料，不寫入專案來源副本。不支援 Makefile/CMake 或多組建置目標。

Imports preserve the external source and reject symlinks, Windows junctions, and special files. Workspace and temporary folders cannot overlap, including physical aliases. Failed copies attempt cleanup only for entries created by that operation whose filesystem identities still match. Unexpected entries and cleanup errors are retained and reported. Copies are not transactions; crashes may leave partial data. Checks cannot prevent another process actively replacing entries during an operation.

匯入保留外部來源，拒絕符號連結、Windows Junction 及特殊檔案。工作區與暫存目錄不可重疊，包括實體路徑別名。複製失敗時，僅清理該次建立且檔案系統身分仍相符的項目；意外新增內容與清理錯誤會保留並回報。複製不是交易，崩潰可能留下部分資料；檢查無法防止其他程序在操作期間持續替換項目。

## Settings / 設定

**Settings** contains workspace, temporary folder, GCC, and GDB choices. Enter or browse absolute folders and choose **Apply folders**. **Use default** stages a default until applied. Defaults are `LearningWorkspace` under Tauri's `appDataDir` and `CodeLearnTemp` under `appCacheDir`. Preferences live in `workspace-settings.json` under `appConfigDir`, separate from both data roots. Invalid saved settings display errors without silently falling back. Changing folders keeps old data and never clears temporary files.

**Settings** 集中設定工作區、暫存目錄、GCC 與 GDB。輸入或瀏覽絕對目錄後按 **Apply folders**；**Use default** 需套用才生效。預設分別是 Tauri `appDataDir` 下的 `LearningWorkspace` 與 `appCacheDir` 下的 `CodeLearnTemp`。偏好保存在 `appConfigDir` 的 `workspace-settings.json`，與資料目錄分離。無效設定會顯示錯誤，不會默默改用預設；切換目錄保留舊資料，也不清空暫存檔。

Use **Find installed tools**, or enter an absolute executable path and choose **Check tool**, then explicitly **Use GCC** or **Use GDB**. GCC must pass a real compile/link check. GDB validation checks version/startup; debugging checks MI3 compatibility at launch. Saved paths are revalidated when the app starts. An invalid GDB does not prevent a valid GCC from building or running. Changing workspace keeps tool selections. Tools are never installed, replaced, or selected automatically, and the system PATH is not changed.

使用 **Find installed tools**，或輸入執行檔絕對路徑後按 **Check tool**，再明確選擇 **Use GCC** 或 **Use GDB**。GCC 須通過真實編譯／連結檢查；GDB 先驗證版本及啟動能力，啟動除錯時再檢查 MI3 相容性。保存的路徑會在啟動時重新驗證；GDB 無效不影響有效 GCC 的建置與執行。切換工作區保留工具選擇，不自動安裝、更換或選定工具，也不修改系統 PATH。

## Editing, building, and running / 編輯、建置與執行

Each file tab keeps its own unsaved changes. Selecting a header does not change compile sources. **Save** saves all changed tabs. Leaving an exercise/project, changing workspace, or closing offers **Save / Discard / Cancel**. Save compares disk content with the last opened or saved content; conflicts and write failures retain your buffer. **Reload file** can discard it after confirmation. Files must be UTF-8; saves replace the destination through a temporary file in the same folder. These checks do not prevent every race with external writers.

每個檔案分頁各自保留未儲存內容；選擇 Header 不會改變編譯來源。**Save** 儲存所有修改過的分頁。離開練習／專案、切換工作區或關閉時，提供 **Save / Discard / Cancel**。儲存前比較磁碟與上次開啟或保存的內容；衝突及寫入失敗時保留編輯內容。**Reload file** 可在確認後放棄編輯。檔案須為 UTF-8，儲存透過同資料夾的暫存檔替換目的檔；無法防止所有外部寫入競合。

**Build**, **Run**, and **Debug** first save all changed files in the current exercise/project. Run and Debug build automatically when the current compilation is missing or no longer valid. Saving or compilation failures stop the action; an older successful program is never substituted. There is only one current compilation. Switching units or changing source files, compile settings, GCC, or workspace invalidates it. Known source contents and the selected GCC file are checked before reuse; there is no file watcher or complete indirect-header tracking.

**Build**、**Run**、**Debug** 先儲存目前練習／專案所有修改過的檔案。Run 與 Debug 在沒有有效編譯結果時自動建置；儲存或編譯失敗即停止，不會改執行舊的成功結果。同時只保留一份目前有效的編譯結果；切換操作單元、修改來源、編譯選擇、GCC 或工作區時使其失效。重用前會核對已掌握來源內容與選定的 GCC 檔案；不提供檔案監看或完整間接 Header 追蹤。

**Standard input** supplies text once at startup, then closes. **Program output** shows final stdout/stderr, outcome, exit code, and errors. Run does not stream output or accept interactive input. Only one program or debugger is active; during Run the editor is read-only. **Stop** remains available during startup and execution. Other operations remain locked until waiting and cleanup finish. Running time is limited to **30 seconds**, with **25 MiB combined** stdout/stderr. Inputs and results stay in memory without history or uploads. Temporary working directories may disappear when their compilation is discarded; runtime input-file selection is not provided.

**Standard input** 於啟動時一次送出文字後關閉。**Program output** 顯示最終 stdout/stderr、結果、結束代碼及錯誤。Run 不即時串流輸出，也不接受互動輸入。同時僅允許一個執行或除錯程序；Run 期間編輯器唯讀。啟動及執行期間仍可按 **Stop**，其他操作須等候程序與清理完成才恢復。執行限制為 **30 秒**，stdout/stderr **合計 25 MiB**。輸入與結果僅留於記憶體，不建立歷史或上傳；編譯結果釋放後，暫存執行目錄可能消失，不提供執行輸入檔選擇。

## Debugging and closing / 除錯與關閉

Choose **Debug** to stop at `main`. Click beside a line number to add/remove a breakpoint. Use **Continue**, **Step into**, **Step over**, **Step out**, or **Pause**. Variables and the call stack are available while paused; selecting a stack frame shows its variables and source when available. Program output and debugger messages are displayed separately. Editing during debugging requests Stop, preserves your input, and waits for cleanup before other operations resume.

選擇 **Debug** 會停在 `main`。點擊行號旁可新增／移除斷點，並使用 **Continue**、**Step into**、**Step over**、**Step out** 或 **Pause**。暫停時可查看變數及呼叫堆疊，選擇堆疊項目可查看該層變數與可用來源。程式輸出與除錯訊息分開顯示。除錯期間編輯會要求停止、保留輸入，並等候清理完成才恢復其他操作。

Closing during execution or debugging requests Stop and waits for completion. Unresolved cleanup problems require **Keep open / Close anyway** confirmation, followed by unsaved-change protection. Closing anyway acknowledges the warning; it does not repair cleanup or prove that every process exited. These controls cover ordinary window closing, not forced OS termination. Student programs have ordinary OS access: process supervision is **not a sandbox**.

執行或除錯期間關閉視窗，會先要求停止並等待完成。尚未解決的清理問題須經 **Keep open / Close anyway** 確認，之後才處理未儲存內容。仍要關閉僅代表確認警告，不會修復清理或證明所有程序已退出。此保護適用一般關閉，不涵蓋作業系統強制終止。學生程式具有一般作業系統存取權，程序管理**不是沙箱**。

## Main dependencies / 主要依賴

| Component / 元件 | Dependencies and purpose / 依賴與用途 |
| --- | --- |
| Desktop UI / 桌面介面 | React, TypeScript, Vite; Monaco Editor for local text editing; `react-markdown` for lesson text / 教材文字; `@tauri-apps/api` for commands and window events / 本機文字編輯、呼叫 Commands 與視窗事件 |
| Desktop host / 桌面宿主 | Tauri 2; `tauri-plugin-dialog` and `@tauri-apps/plugin-dialog` for native selection / 原生選擇視窗 |
| Workspace / 工作區 | `serde`, `serde_json` for local preferences; `tempfile` for settings/file replacement and write probes; `same-file` for cleanup ownership / 本機偏好、設定／檔案替換與寫入檢查、清理歸屬檢查 |
| Filename transport / 檔名傳遞 | `percent-encoding` preserves native filename bytes between the UI and Rust / 保留 UI 與 Rust 之間傳遞的原始檔名位元組 |
| Rust Core direct dependencies / Rust Core 直接依賴 | `same-file`, `tempfile`; Linux `libc`; Windows `windows-sys` for process and filesystem APIs / 程序及檔案系統 API |
| External toolchains / 外部工具鏈 | GCC and GDB are not bundled or automatically installed / GCC 與 GDB 不隨 Desktop 內建，也不自動安裝 |

Permissions allow native file selection and window close confirmation for unsaved changes. The dialog plugin brings `tauri-plugin-fs` transitively; Desktop does not expose frontend filesystem permissions or use it for workspace operations. Monaco and its worker are bundled locally without a CDN. Its styles require inline styles in the content security policy; script sources remain restricted. Monaco is MIT-licensed; exact dependency versions are recorded in the Rust and npm lockfiles.

權限僅提供原生檔案選擇，以及未儲存內容的關閉確認。Dialog Plugin 間接依賴 `tauri-plugin-fs`；Desktop 未開放前端檔案系統權限，也不用它處理 Workspace 操作。Monaco 及其背景工作程式隨應用程式於本機載入，不使用 CDN；其樣式需要內容安全政策允許行內樣式，腳本來源仍受限制。Monaco 採 MIT 授權；確切相依版本記錄於 Rust 與 npm Lockfile。

## Repository / 專案結構

```text
crates/codelearn-tool-core/   # UI-independent Rust library / 獨立 Rust 核心
apps/desktop/                # React + TypeScript + Vite
  src-tauri/                 # Independent Tauri package / 獨立 Tauri 套件
.github/workflows/
  ci.yml                     # Automatic checks / 自動檢查
  build-desktop.yml          # Manual desktop builds / 手動桌面建置
Cargo.toml                   # Core workspace; excludes Tauri / 排除 Tauri
Cargo.lock                   # Core dependency lockfile / Core 相依鎖定檔
```

Tauri depends on Core through a local path. The root and Tauri each have their own tracked `Cargo.lock`; frontend dependencies use `apps/desktop/package-lock.json`.

Tauri 透過本機路徑依賴 Core。根目錄與 Tauri 各自追蹤獨立的 `Cargo.lock`；前端使用 `apps/desktop/package-lock.json`。

## Development / 開發

Use Node.js **24**, npm, and stable Rust with rustfmt and Clippy. Target Windows x64 or Linux x64. Install the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your platform before building.

使用 Node.js **24**、npm，以及含 rustfmt 和 Clippy 的 stable Rust，目標平台為 Windows x64 或 Linux x64。建置前請安裝對應平台的 [Tauri 必要元件](https://v2.tauri.app/start/prerequisites/)。

- **Windows:** MSVC Rust toolchain, Visual Studio C++ Build Tools with the Windows SDK, and Microsoft Edge WebView2 Runtime.
- **Windows：** MSVC Rust 工具鏈、包含 Windows SDK 的 Visual Studio C++ Build Tools，以及 Microsoft Edge WebView2 Runtime。
- **Linux:** Ubuntu 24.04 x64 is the packaging baseline. Install the development packages below to build the desktop shell. Core compilation features require a separately available GCC; the desktop does not install toolchains.
- **Linux：** 封裝基準為 Ubuntu 24.04 x64。請安裝下列開發套件以建置桌面程式。Core 編譯功能需另有可用的 GCC；桌面程式不安裝工具鏈。

```sh
sudo apt-get update
sudo apt-get install --no-install-recommends -y libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev libfuse2t64
```

From the repository root / 從專案根目錄執行：

```sh
cargo build --workspace --locked
cd apps/desktop
npm ci
npm run tauri -- dev
```

`npm run dev` starts only the frontend preview at `http://127.0.0.1:5173`; `npm run tauri -- dev` starts the desktop window as well.

`npm run dev` 僅啟動位於 `http://127.0.0.1:5173` 的前端預覽；`npm run tauri -- dev` 會一併啟動桌面視窗。

## Checks / 檢查

From the repository root / 從專案根目錄執行：

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --locked
```

From `apps/desktop` / 從 `apps/desktop` 執行：

```sh
npm ci
npm run typecheck
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --locked --manifest-path src-tauri/Cargo.toml
cargo build --locked --manifest-path src-tauri/Cargo.toml
```

Build the frontend before direct Cargo checks of Tauri. No frontend test suite is currently defined. Desktop workspace tests cover persistence, root and link boundaries, import/copy protection, New File restrictions, and failed-copy cleanup using synthetic temporary files. Default Core tests cover path boundaries, artifact cleanup errors (Windows), and compiler process supervision without requiring GCC/GDB. The Windows path-alias test creates temporary junctions and requires an environment permitting that operation.

直接執行 Tauri 的 Cargo 檢查前，請先建置前端。目前未建立前端測試套件。Desktop Workspace 測試使用暫存合成檔案，涵蓋設定保存、根目錄與連結邊界、匯入／複製保護、New File 限制，以及複製失敗清理。Core 預設測試涵蓋路徑邊界、產物清理錯誤（Windows）及編譯程序管理，不需要 GCC/GDB。Windows 路徑別名測試會建立暫存 Junction，執行環境須允許此操作。

Desktop tests also cover UTF-8 read/save, external-change conflicts, unsaved edits, source boundaries, and rejection of conflicting operations. A separate opt-in Desktop compilation test uses an existing GCC to check single/multiple sources, warnings, failed builds, and result invalidation without running compiled programs. From `apps/desktop`, set `CODELEARN_TEST_GCC` to an absolute GCC path and run `cargo test --locked --manifest-path src-tauri/Cargo.toml desktop_compile_tracks -- --ignored --nocapture`; optionally provide `CODELEARN_TEST_GDB` to check GDB selection too. Native editor interactions still require GUI validation on Windows and Linux.

Desktop 測試也涵蓋 UTF-8 讀寫、外部修改衝突、未儲存編輯、來源邊界，以及衝突操作的拒絕。另有需明確啟用的 Desktop 編譯測試，使用既有 GCC 驗證單檔／多檔、警告、失敗編譯及結果失效，不執行編譯產出的程式。在 `apps/desktop` 設定 `CODELEARN_TEST_GCC` 為 GCC 絕對路徑，再執行 `cargo test --locked --manifest-path src-tauri/Cargo.toml desktop_compile_tracks -- --ignored --nocapture`；亦可提供 `CODELEARN_TEST_GDB` 檢查 GDB 選擇。原生編輯器互動仍須於 Windows 與 Linux 進行 GUI 驗證。

Course/project tests cover manifest validation, version separation, exercise copies/reset, local image boundaries, and saved project sources. To check the Desktop execution/debug connection with synthetic programs, set both `CODELEARN_TEST_GCC` and `CODELEARN_TEST_GDB` to existing absolute executable paths and run `cargo test --locked --manifest-path src-tauri/Cargo.toml desktop_ -- --ignored --nocapture` from `apps/desktop`. These tests execute local synthetic programs. GUI validation still requires a representative course and project flow on each platform; build success does not confirm GUI compatibility.

課程／專案測試涵蓋格式驗證、版本隔離、練習副本／重設、本機圖片邊界及編譯來源保存。若要以合成程式驗證 Desktop 執行／除錯接線，設定 `CODELEARN_TEST_GCC` 與 `CODELEARN_TEST_GDB` 為既有執行檔絕對路徑，再於 `apps/desktop` 執行 `cargo test --locked --manifest-path src-tauri/Cargo.toml desktop_ -- --ignored --nocapture`。這些測試會執行本機合成程式。兩個平台仍須各完成代表性的課程與專案 GUI 流程，建置成功不代表介面相容性已驗證。

Real GCC checks are opt-in: set `CODELEARN_TEST_GCC` to an existing absolute GCC path, then run `cargo test --workspace --locked --test build -- --ignored --nocapture`. Optionally set `CODELEARN_TEST_GDB` to check a real GDB, and `CODELEARN_TEST_REQUIRE_PATH=1` to require the selected GCC to also be discovered through the current PATH. These checks compile synthetic sources without executing the resulting programs. Tools are never installed automatically.

真實 GCC 檢查須明確啟用：將 `CODELEARN_TEST_GCC` 設為既有 GCC 的絕對路徑，再執行 `cargo test --workspace --locked --test build -- --ignored --nocapture`。可選擇設定 `CODELEARN_TEST_GDB` 以檢查真實 GDB；設定 `CODELEARN_TEST_REQUIRE_PATH=1` 則要求選定的 GCC 亦可從目前 PATH 偵測到。這些檢查只編譯合成來源，不執行產出的程式，也不會自動安裝工具。

To validate execution with synthetic C programs, set `CODELEARN_TEST_GCC` as above and run `cargo test --workspace --locked --test run -- --ignored --nocapture`. Three grouped integration tests cover raw I/O and selected input files, path/start failures and artifact lifetime, stop/timeout/drop and descendants, and the combined output boundary. The Linux cleanup check also uses a bounded descendant that deliberately leaves its process group. The existing conditional Core integration steps run these checks on Windows and Linux using their existing toolchains; documentation-only and UI-only changes do not enable them.

若要以合成 C 程式驗證執行功能，依上述方式設定 `CODELEARN_TEST_GCC`，再執行 `cargo test --workspace --locked --test run -- --ignored --nocapture`。三組整合測試涵蓋原始 I/O 與指定輸入檔、路徑／啟動失敗與 Artifact 生命週期、停止／逾時／釋放與後代程序，以及共用輸出額度的邊界。Linux 清理檢查另使用會自行結束、刻意離開程序群組的合成後代。既有條件式 Core 整合步驟會使用原有工具鏈，在 Windows 與 Linux 執行這些檢查；純文件或 UI 修改不會啟用它們。

For real debug checks, select both `CODELEARN_TEST_GCC` and `CODELEARN_TEST_GDB`, then run `cargo test --workspace --locked --test debug -- --ignored --nocapture`. Three grouped tests cover session operations, I/O and limits, and process/resource lifetime. They run in the existing conditional Core toolchain steps on both platforms. A Linux container needs an init that reaps orphaned exited processes, as a normal desktop host does; a reaper used by a local test environment is not a Core dependency or a substitute for Core terminating the processes.

真實除錯檢查須同時選定 `CODELEARN_TEST_GCC` 與 `CODELEARN_TEST_GDB`，再執行 `cargo test --workspace --locked --test debug -- --ignored --nocapture`。三組測試涵蓋 Session 操作、I/O 與限制，以及程序／資源生命週期，並整合至兩平台既有的條件式 Core 工具鏈步驟。Linux 容器需如一般桌面宿主般，具備可回收已退出孤兒程序的 Init；本機測試環境的 Reaper 不是 Core 相依，也不能取代 Core 終止程序的責任。

## Desktop builds / 桌面建置

Run in `apps/desktop` after `npm ci` / 在 `apps/desktop` 完成 `npm ci` 後執行：

| Platform / 平台 | Command / 指令 | Output / 產物 |
| --- | --- | --- |
| Windows x64 | `npm run tauri -- build --no-bundle -- --locked` | `src-tauri/target/release/CodeLearn Tool.exe` |
| Linux x64 | `npm run tauri -- build --bundles appimage -- --locked` | `src-tauri/target/release/bundle/appimage/*.AppImage` |

These commands build the frontend automatically. Windows produces a standalone EXE without an installer; Linux produces an AppImage without a `.deb` package.

這些指令會自動建置前端。Windows 產生無安裝程式的獨立 EXE；Linux 產生 AppImage，不產生 `.deb`。

### Runtime limits / 執行限制

The Windows x64 EXE requires Microsoft Edge WebView2 Runtime on the destination machine; it does not install or bundle that runtime. Use a supported Windows environment with a working WebView2 installation.

Windows x64 EXE 需要目標電腦已安裝 Microsoft Edge WebView2 Runtime；EXE 不包含或安裝此執行環境。請使用具備可正常運作 WebView2 的受支援 Windows 環境。

The Linux AppImage is built on **Ubuntu 24.04 x64**. It is not guaranteed to run on older distributions or every Linux desktop; glibc, graphics, WebKitGTK-related libraries, and AppImage/FUSE support affect compatibility. Grant execute permission with `chmod +x <file>.AppImage` before launching. See [Tauri's AppImage notes](https://v2.tauri.app/distribute/appimage/).

Linux AppImage 以 **Ubuntu 24.04 x64** 建置，不保證適用舊版發行版或所有 Linux 桌面；相容性受 glibc、圖形環境、WebKitGTK 相關函式庫及 AppImage/FUSE 支援影響。啟動前請執行 `chmod +x <file>.AppImage` 賦予執行權限。請參閱 [Tauri AppImage 說明](https://v2.tauri.app/distribute/appimage/)。

A successful build or an uploaded artifact does not establish desktop runtime compatibility. Validate the GUI on each intended platform. AppImage packaging is not a security sandbox.

建置成功或 Artifact 上傳成功不代表桌面執行相容性已驗證；仍須在各目標平台實際啟動 GUI。AppImage 封裝不是安全沙箱。

## GitHub Actions / 自動化流程

`CI` runs on pushes to `main` and pull requests: frontend install/typecheck/build once on Ubuntu, plus Core and Tauri Clippy/test/build on Windows and Linux. Format checks run only on Linux. The frontend build is shared with Tauri jobs as a one-day internal artifact; CI does not publish desktop downloads.

`CI` 在 Push 至 `main` 或 Pull Request 時執行：前端安裝、型別檢查及建置只在 Ubuntu 執行一次，Core 與 Tauri 的 Clippy／Test／Build 分別在 Windows 及 Linux 執行。Format 僅在 Linux 檢查。前端產物透過保留一天的內部 Artifact 供 Tauri Job 使用；CI 不發布桌面下載產物。

For downloads, open **Actions → Build Desktop → Run workflow** and select **Windows**, **Linux**, or **Both**. Both runs two independent jobs. Artifacts are checked for existence and nonzero size, named `codelearn-tool-windows-x64-<short-sha>` or `codelearn-tool-linux-x64-<short-sha>`, and retained for **30 days**. This manual workflow does not create releases or tags.

如需下載產物，開啟 **Actions → Build Desktop → Run workflow**，選擇 **Windows**、**Linux** 或 **Both**；Both 會執行兩個獨立 Job。上傳前會確認產物存在且非空，名稱為 `codelearn-tool-windows-x64-<short-sha>` 或 `codelearn-tool-linux-x64-<short-sha>`，保留 **30 天**。此手動流程不建立 Release 或 Tag。

## License / 授權

[MIT](LICENSE). Third-party dependencies retain their respective licenses.

[MIT](LICENSE)。第三方相依套件仍適用各自授權。

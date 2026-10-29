# CodeLearn Tool

A Windows and Linux desktop application for learning programming, starting with C. It is intended for focused learning, rather than replacing a full IDE.

面向 Windows 與 Linux 的程式學習桌面應用程式，從 C 語言開始，以學習為主，不以取代完整 IDE 為目標。

## Current scope / 目前範圍

The Tauri 2 desktop now provides Workspace Settings, Imported/Authored file browsing, file and folder import, Copy to Authored, and New File (`.c`, `.h`, `.txt`). The editor opens and saves UTF-8 `.c`, `.h`, and `.txt` files in Authored using Monaco. Select a validated GCC and explicit sources to compile C11 programs and read compiler output, then run the current compilation with optional standard input and Stop control. The independent Rust Core also provides supervised program execution and GDB/MI3 debugging APIs.

Tauri 2 桌面程式現已提供 Workspace Settings、Imported／Authored 檔案瀏覽、單檔與整個資料夾匯入、Copy to Authored，以及 New File（`.c`、`.h`、`.txt`）。編輯器使用 Monaco 開啟及儲存 Authored 內的 UTF-8 `.c`、`.h`、`.txt` 檔案。選定經驗證的 GCC 與明確的來源檔，即可編譯 C11 程式並檢視編譯輸出，再執行目前編譯結果，提供選用的標準輸入及 Stop 操作。獨立 Rust Core 也提供受控程序執行及 GDB/MI3 除錯 API。

Toolchain installation, desktop debug controls, lessons, and learning progress are **not implemented**. Workspace files and local path preferences persist on this machine. No telemetry, cloud services, or learning-record storage is included.

工具鏈安裝、桌面除錯操作、課程及學習進度均**尚未實作**。Workspace 檔案與本機路徑偏好會保存在這台電腦；不包含遙測、雲端服務或學習紀錄儲存。

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

## Desktop workspace / 桌面工作區

Learning Workspace defaults to `LearningWorkspace` under Tauri's `appDataDir`; CodeLearnTemp defaults to `CodeLearnTemp` under `appCacheDir`. Browse or enter absolute directories, then Apply. The active resolved paths are shown below the inputs. Restore Default stages the default for that path; Apply saves it. JSON preferences live in `workspace-settings.json` under `appConfigDir`, separate from both data roots. Invalid saved settings display an error and can be corrected here; the app does not silently fall back to defaults. Changing roots leaves old files in place and never clears Temp.

Learning Workspace 預設位於 Tauri `appDataDir` 下的 `LearningWorkspace`；CodeLearnTemp 預設位於 `appCacheDir` 下的 `CodeLearnTemp`。可用 Browse 或輸入絕對目錄，再按 Apply；輸入欄下方顯示目前生效的解析路徑。Restore Default 會先將該路徑改回預設選項，按 Apply 才保存。JSON 偏好儲存於 `appConfigDir` 下的 `workspace-settings.json`，與兩個資料根目錄分開。已保存的設定無效時會顯示錯誤，可在此修正，不會默默改用預設。切換目錄保留舊位置的檔案，也不清空 Temp。

Import File/Folder copies all regular files to Imported, preserving the folder structure and the external source. Select an entry and use Copy to Authored to create an independent working copy, retaining its path relative to Imported. Use Open/Up to browse folders; New File creates an empty `.c`, `.h`, or `.txt` in the currently displayed Authored folder. Existing destinations are rejected without overwrite or folder merging. Imports and copies reject symlinks, Windows junctions, and special files; links in existing areas cannot be opened. Workspace and Temp cannot overlap, including through physical path aliases. Operations are serialized and errors are displayed in the window.

Import File／Folder 將所有一般檔案複製至 Imported，保留目錄結構與外部原始檔。選取項目後按 Copy to Authored，即可建立獨立的工作副本，保留相對於 Imported 的路徑。使用 Open／Up 瀏覽資料夾；New File 會在目前顯示的 Authored 資料夾建立空白 `.c`、`.h` 或 `.txt`。既有目的項目會被拒絕，不覆寫或合併資料夾。匯入與複製拒絕 Symlink、Windows Junction 及特殊檔案，既有區域中的連結無法開啟。Workspace 與 Temp 不可重疊，包括實體路徑別名。操作依序處理，錯誤顯示於視窗。

On Linux, imported names containing colons, backslashes, or non-UTF-8 bytes remain browsable and can be copied to Authored. Bytes that cannot be displayed as text appear as `�`; file operations preserve the original names, including names that look identical in the list.

在 Linux，匯入檔名含冒號、反斜線或非 UTF-8 位元組時，仍可瀏覽及複製至 Authored。無法以文字顯示的位元組會顯示為 `�`；檔案操作保留原始檔名，即使列表中的名稱看起來相同也不會混淆。

Failed copies attempt to remove only entries created by that operation whose filesystem identities still match. Unexpected entries or cleanup errors are retained and reported. File copies are not transactions; a crash can leave partial data. Path checks do not protect against another process actively replacing entries during an operation.

複製失敗時，僅嘗試移除該次建立、檔案系統身分仍相符的項目；意外新增的內容或無法清理的項目會保留並回報。檔案複製不是交易，崩潰可能留下部分資料；路徑檢查無法防範其他程序在操作期間持續替換檔案項目。

## Editing and compiling / 編輯與編譯

Choose **Edit** beside an Authored `.c`, `.h`, or `.txt` file. The editor shows unsaved changes and provides Save and Reload. Opening another file, applying workspace settings, or closing with unsaved changes offers Save, Discard, and Cancel. Save compares the on-disk content with the last opened or saved content; a conflict or write failure keeps your edits open. To abandon edits after an external change, choose Reload and then Discard. Non-UTF-8 content is rejected without conversion; native Linux filenames remain supported. Saves use a temporary file in the same folder before replacing the destination. These checks do not prevent every race with external writers.

在 Authored 的 `.c`、`.h` 或 `.txt` 檔案旁選擇 **Edit**。編輯器顯示未儲存狀態，並提供 Save 與 Reload。開啟其他檔案、套用工作區設定或關閉視窗時，若有未儲存內容，會提供 Save、Discard、Cancel。儲存前會將磁碟內容與上次開啟或儲存的內容比對；衝突或寫入失敗時保留編輯內容。外部修改後若要放棄自己的編輯，選 Reload，再選 Discard。不符合 UTF-8 的內容會被拒絕，不進行轉碼；仍支援 Linux 原始檔名。儲存時先寫入同資料夾的暫存檔，再替換目的檔案；這些檢查無法防止所有外部同時寫入的競合。

Use **Scan / Refresh**, or enter an absolute executable path and choose **Validate**, then explicitly **Select GCC** or **Select GDB**. Validation shows the version, availability, origin, and failure details. GCC must pass a real compile/link check. GDB is checked independently for version/startup; it is optional for compilation and this check does not establish debugging compatibility. Selections are kept only while the app is open. No tools are automatically selected or installed.

使用 **Scan / Refresh**，或輸入執行檔絕對路徑後選 **Validate**，再明確按下 **Select GCC** 或 **Select GDB**。驗證結果顯示版本、可用性、來源及失敗細節。GCC 必須通過真實編譯／連結檢查；GDB 另行檢查版本與啟動能力，編譯時不必選擇 GDB，這項檢查也不代表除錯相容性已驗證。工具選擇只保留至應用程式關閉，不自動選擇或安裝。

Select an Authored entry in the file list, then use **Set Primary .c**, **Add Other .c**, or **Add Include Folder**. **Include Current Folder** adds the currently displayed Authored folder. Sources are separate from the open editor file, so you can edit a header while compiling selected C sources. Imported files must first be copied to Authored. Compilation uses only the explicitly selected sources and include directories, all within Authored. **Save & Compile** saves current edits first and stops if saving fails. The interface remains responsive during compilation, while editing, file operations, settings, and tool selection are temporarily unavailable.

先在檔案列表選取 Authored 項目，再用 **Set Primary .c**、**Add Other .c** 或 **Add Include Folder** 加入編譯來源；**Include Current Folder** 加入目前顯示的 Authored 資料夾。編譯來源與編輯中的檔案分開，因此可編輯 Header，同時編譯指定的 C 來源。Imported 檔案須先複製至 Authored。編譯只使用明確選取、且位於 Authored 內的來源及 Include 目錄。**Save & Compile** 會先儲存目前編輯內容，儲存失敗即停止。編譯期間介面仍可回應，但暫時禁止編輯、檔案操作、設定及工具選擇。

Compile Output shows success/failure, exit code, standard output, warnings/errors, and cleanup problems. Warnings can accompany success. Editing or saving, changing sources/includes/GCC/workspace, or starting another compilation invalidates the previous result. A failed compilation never restores an older successful result. Known source contents are checked before and after compilation and when refreshing the displayed state. There is no file watcher or complete indirect-header dependency tracking; files changed outside the app may remain undetected until a check. The app retains the compiled result locally only while it remains current. Execution requires an explicit Run action.

Compile Output 顯示成功／失敗、結束代碼、標準輸出、警告／錯誤及清理問題；有警告仍可編譯成功。編輯或儲存、變更來源／Include／GCC／工作區，或開始下一次編譯，都會使舊結果失效。編譯失敗不會恢復使用先前的成功結果。編譯前後與更新顯示狀態時，會檢查已掌握來源的內容；沒有檔案監看或完整間接 Header 相依追蹤，外部變更可能直到檢查時才被發現。應用程式僅在結果仍有效時於本機保留編譯產物，須明確按下 Run 才會執行。

## Running a program / 執行程式

After a successful compilation, enter any UTF-8 text in **Standard input** and choose **Run**. Run checks the saved sources and current compilation again; unsaved edits, changed known sources, or a missing executable require compilation first. Run never compiles automatically. Only one program runs at a time. The editor is read-only and file, settings, toolchain, and compilation operations remain unavailable until execution and cleanup finish. **Stop** remains available, including during startup; requesting stop does not mean cleanup has finished.

成功編譯後，可在 **Standard input** 預先輸入 UTF-8 文字，再按 **Run**。執行前會重新核對已儲存來源與目前編譯結果；有未儲存修改、已掌握來源變更或執行檔遺失時，必須先編譯，不會自動編譯。同時僅執行一個程式；直到執行與清理完成前，編輯器暫時唯讀，檔案、設定、工具選擇及編譯操作暫停。**Stop** 在啟動期間亦可使用；提出停止要求不代表清理已完成。

Input closes after the supplied text is sent; there is no interactive input. **Run Output** displays the final outcome, exit code, stdout, stderr, start/process errors, cleanup errors, and temporary working directory separately from Compile Output. Nonzero exit codes and text on stderr can be ordinary program results. Non-UTF-8 output bytes use replacement characters. The time limit is **30 seconds**, and stdout/stderr have a **25 MiB combined** limit. The working directory may disappear when its compilation is invalidated. Imported files are not automatically copied there; runtime input-file selection is not provided. Input and the latest result remain in memory only, without run history or uploads.

預先文字送出後即關閉輸入，不提供執行中互動輸入。**Run Output** 與 Compile Output 分開，完成後顯示執行結果、結束代碼、stdout、stderr、啟動／程序錯誤、清理錯誤及暫存工作目錄。非零結束代碼或 stderr 文字可能是一般程式結果；非 UTF-8 輸出以替代字元顯示。時間限制為 **30 秒**，stdout/stderr **合計上限 25 MiB**。編譯結果失效時，暫存工作目錄可能被移除。Imported 檔案不會自動複製至此，也不提供執行輸入檔選擇。輸入與最近結果僅保留於記憶體，不建立執行歷史或上傳資料。

Closing the window during a run requests Stop and waits for the final result. Cleanup problems keep the window open for **Keep Open / Close Anyway** confirmation, followed by the usual Save / Discard / Cancel protection for unsaved edits. A later successful run does not erase unresolved cleanup warnings. Closing anyway acknowledges the warning; it does not repair cleanup or prove that all processes exited. These controls cover normal window closing, not forced OS termination, and do not provide a sandbox.

執行中關閉視窗會先要求 Stop，並等待最終結果。若有清理問題，視窗會保留並提供 **Keep Open / Close Anyway** 確認，之後才進入既有的 Save／Discard／Cancel 未儲存保護。後續成功執行不會抹除尚未解決的清理警告。選擇仍要關閉只是確認警告，不會修復清理問題，也不代表所有程序皆已結束。此流程適用一般視窗關閉，不涵蓋作業系統強制終止，亦不提供沙箱隔離。

## Main dependencies / 主要依賴

| Component / 元件 | Dependencies and purpose / 依賴與用途 |
| --- | --- |
| Desktop UI / 桌面介面 | React, TypeScript, Vite; Monaco Editor for local text editing; `@tauri-apps/api` for commands and window events / 本機文字編輯、呼叫 Commands 與視窗事件 |
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

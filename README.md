# CodeLearn Tool

A Windows and Linux desktop application for learning programming, starting with C. It is intended for focused learning, rather than replacing a full IDE.

面向 Windows 與 Linux 的程式學習桌面應用程式，從 C 語言開始，以學習為主，不以取代完整 IDE 為目標。

## Current scope / 目前範圍

The application currently displays a minimal welcome window. It uses an independent Rust Core library and a Tauri 2 desktop shell with React, TypeScript, and Vite. Core has no business APIs or UI dependencies yet.

目前應用程式僅顯示最小歡迎視窗，採用獨立 Rust Core Library，以及 Tauri 2、React、TypeScript、Vite 桌面架構。Core 尚無業務 API，也不依賴 UI。

C editing, GCC/GDB discovery or installation, compile/run/debug, lessons, and learning progress are **not implemented**. No telemetry, cloud services, or student-data persistence is included.

C 程式編輯、GCC/GDB 偵測或安裝、編譯／執行／除錯、課程及學習進度均**尚未實作**。目前不包含遙測、雲端服務或學生資料儲存。

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
- **Linux:** Ubuntu 24.04 x64 is the packaging baseline. Install the development packages below; these build the desktop shell and do not provide application-level GCC/GDB integration.
- **Linux：** 封裝基準為 Ubuntu 24.04 x64。請安裝下列開發套件；這些套件供桌面程式建置使用，不代表應用程式已具備 GCC/GDB 整合。

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

Build the frontend before direct Cargo checks of Tauri. No frontend test suite is currently defined; the Rust test commands currently discover no tests because no business logic is implemented.

直接執行 Tauri 的 Cargo 檢查前，請先建置前端。目前未建立前端測試套件；由於尚無業務邏輯，Rust 測試指令目前不會找到測試案例。

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

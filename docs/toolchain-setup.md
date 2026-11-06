# Toolchain setup / 工具鏈設定

## English

Use **Settings → Compiler and debugger → Find installed tools** first. Working
GCC/GDB tools can come from MSYS2 or another installation. A tool is selected and
saved only when you click **Use GCC** or **Use GDB**. Startup validates saved tools
again; setup checks never choose a replacement automatically.

On Windows, **Toolchain setup → Check MSYS2** checks `C:\msys64` by default. Use
**Browse** or enter another absolute MSYS2 root directory. It does not scan drives.
The check requires the UCRT64 directory, pacman executable, MSYS runtime, pacman
configuration, and local package database within that root. Network/device roots,
components resolving outside the root, incomplete layouts, and an existing
package database lock are rejected. Layout and version checks are consistency
checks, not cryptographic proof that an executable is official; choose a trusted
installation.

The result distinguishes packages installed in this MSYS2 from working tools
already known to CodeLearn. A tool is checked by default only if neither is known.
Choose GCC and GDB individually, then **Refresh package preview**. The advisory
table shows repository names, versions, new dependencies, and any packages already
installed. Existing packages are always marked as prohibited changes, including
same-version reinstalls. Changing the path or selection invalidates the old
preview. Errors include details and a retry action.

**There is no Install action.** Even an all-new preview is insufficient to
authorize a transaction. The app does not refresh package databases, download,
install, update, replace, remove, repair, elevate privileges, or change the system
PATH. Only the child process receives a local MSYS2 PATH and UCRT64 environment.
Read-only queries have a 30-second limit and use the existing process supervisor;
this does not establish that terminating a package installation would be safe.

For a missing or outdated MSYS2, copy the official addresses shown in Settings
into your browser: [installation](https://www.msys2.org/) and
[updating](https://www.msys2.org/docs/updating/). Install or update manually using
those instructions, then return and check again. The app reuses its existing
discovery and validation to show candidates from the selected UCRT64 directory.
GCC must compile/link a probe; GDB must pass its version/startup check. Package
presence alone is not proof of usability. Choose **Use GCC / Use GDB** explicitly
to save a working candidate. Linux keeps its existing manual tool selection;
MSYS2 setup is Windows-only.

Settings also shows manual package commands. After completing the official update
instructions, run the command for each needed tool in an **MSYS2 UCRT64** terminal:

```sh
pacman -S --needed mingw-w64-ucrt-x86_64-gcc
pacman -S --needed mingw-w64-ucrt-x86_64-gdb
```

Review pacman's proposed changes before confirming: manual operations can change
existing packages. These commands are guidance only; the app never executes them.

## 繁體中文

請先使用 **Settings → Compiler and debugger → Find installed tools**。可用的
GCC/GDB 可以來自 MSYS2 或其他安裝來源，只有明確按 **Use GCC** 或 **Use GDB**
才選用並保存。啟動時會重新驗證已保存的工具；環境檢查不會自動改選其他工具。

Windows 的 **Toolchain setup → Check MSYS2** 預設檢查 `C:\msys64`，可用
**Browse** 或輸入另一個 MSYS2 根目錄的絕對路徑，不會掃描整碟。指定根目錄內須有
UCRT64 目錄、pacman 執行檔、MSYS runtime、pacman 設定及本機套件資料庫。
網路／裝置路徑、解析至根目錄外的元件、不完整環境及已存在的套件資料庫鎖都會被拒絕。
目錄結構與版本檢查只能檢查一致性，不是執行檔官方身分的密碼學證明；請選擇可信的安裝。

結果會區分這個 MSYS2 的已安裝套件，以及 CodeLearn 已知的可用工具；兩者皆無時
才預設勾選該工具。可個別選擇 GCC/GDB，再按 **Refresh package preview**。
供檢視的表格會列出套件來源、版本、新增相依及既有套件；任何既有套件變更均標示為
禁止，包含相同版本重裝。修改路徑或選項會使舊預覽失效，錯誤提供詳細資訊及重試入口。

**目前沒有 Install 操作。** 即使預覽全部是新套件，也不足以授權實際安裝。程式
不會同步套件資料庫、下載、安裝、更新、替換、移除、修復、自行升權或變更系統 PATH。
僅子程序會使用該 MSYS2 的 PATH 與 UCRT64 環境。唯讀查詢設有 30 秒上限，沿用
既有程序監督；這不代表中止真正的套件安裝是安全的。

若 MSYS2 未安裝或過舊，請將 Settings 中的官方網址複製至瀏覽器，依
[安裝說明](https://www.msys2.org/)與[更新說明](https://www.msys2.org/docs/updating/)
手動處理，再回程式重新檢查。程式會沿用既有搜尋與驗證，顯示指定 UCRT64 目錄的候選。
GCC 須通過實際編譯／連結探測；GDB 須通過版本／啟動檢查。套件存在不代表工具可用，
仍須明確按 **Use GCC / Use GDB** 才保存選擇。Linux 維持既有手動工具選擇；
MSYS2 環境檢查僅提供於 Windows。

Settings 也提供手動套件指令。依官方說明完成更新後，在 **MSYS2 UCRT64** 終端機
執行所需工具的指令：

```sh
pacman -S --needed mingw-w64-ucrt-x86_64-gcc
pacman -S --needed mingw-w64-ucrt-x86_64-gdb
```

確認前須自行檢視 pacman 提出的變更，因為手動操作可能修改既有套件。
上述指令僅供參考，程式不會執行它們。

## Installation safety boundary / 安裝安全界線

The CLI preview cannot establish the required add-only transaction contract:

- pacman's [`--print` implementation](https://github.com/msys2/msys2-pacman/blob/master/src/pacman/pacman.c)
  enables `NOCONFLICTS` and `NOLOCK`: its checks differ from installation and it
  cannot hold the reviewed state until a separate install command.
- The [`sync` implementation](https://github.com/msys2/msys2-pacman/blob/master/src/pacman/sync.c)
  prints the transaction's **add** list and releases the transaction without
  committing it. This is not a complete machine-readable removal/replacement plan.
- [`--needed`](https://github.com/msys2/msys2-pacman/blob/master/doc/pacman.8.asciidoc)
  skips up-to-date targets; it does not forbid upgrading installed packages.
- MSYS2 [supports full system upgrades](https://www.msys2.org/docs/updating/).
  Cached repository metadata cannot establish current version consistency.
  Installation scripts/hooks and concurrent changes also require guarantees a
  separate preview does not provide.

Therefore this implementation stops at read-only assessment and manual guidance.
It does not add installation lifecycle or cancellation/close controls for an
unavailable operation. Enabling installation requires a separately verified
transaction mechanism that reliably enforces the existing no-update,
no-replacement, no-removal and prior-dependency-confirmation requirements; parsing
a successful preview or automatically answering pacman prompts is insufficient.

CLI 預覽無法建立必要的「只新增」交易保證：`--print` 會啟用 `NOCONFLICTS`／
`NOLOCK`，檢查方式與真正安裝不同，且無法將已檢視狀態鎖定至另一個安裝命令；
`sync` 只輸出交易的新增清單，隨後釋放交易，不是完整且可機器判讀的移除／替換計畫。
`--needed` 只略過最新套件，不能禁止升級舊套件。MSYS2 僅支援完整系統更新，
快取資料庫也不能證明目前版本一致性；安裝 scripts/hooks 與其他程序並行變更
亦需要額外保證。

因此本實作停在唯讀評估與手動引導，沒有為尚不可用的安裝操作新增安裝生命週期、
取消或關閉控制。未來若要啟用安裝，須另外驗證能可靠執行禁止更新／替換／移除，
以及事前確認新增相依的交易機制；成功解析預覽或自動回答 pacman 提示都不足以放行。

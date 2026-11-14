# 15｜標頭檔與多檔程式

> 從單檔走向模組化。教材以繁體中文說明，C 關鍵字與程式名稱保留英文。

## 學習目標

- 辨認 `.h` 宣告、`.c` 實作與 `#include` 的分工。
- 理解一個 Program Block 可以包含多個 `.c/.h`。
- 確保編譯目標包含所有需要連結的 `.c` 檔案。

## 核心觀念

當程式變大時，可以把函式定義放在另一個 `.c` 原始碼檔，並在 `.h` 標頭檔提供函式宣告。`#include "utils.h"` 將該宣告提供給呼叫端；真正的實作仍在 `utils.c`。

本課的 Program Block 已提供 `main.c`、`utils.c`、`utils.h`，編譯來源包含 `main.c` 和 `utils.c`。Courses 不允許你在練習中任意新增、刪除或重新命名檔案，因此請修改已提供的檔案即可。

編譯（compile）和連結（link）不是同一步驟：缺少需要的 `.c` 實作時，可能出現未定義參照的連結錯誤。標頭的 include guard（`#ifndef` 等）可避免宣告在同個翻譯單元內重複展開。

## 先預測，再實作

先觀察 `main.c` 如何呼叫 `sum_three`，再開啟 `utils.c`，把第三個參數改成乘以 2 後回傳，對照輸出。還原時可使用 `Reset exercise`，只恢復此 Program 提供的檔案。

請按本章的 **Open exercise** 開啟練習，修改已提供的檔案，再使用 **Run** 觀察 **Program output**。Markdown 中的程式碼僅供閱讀，不會直接執行。`Run` 前請依現有流程保存程式碼；若缺少 GCC 可先閱讀課文，之後到 **Settings** 選擇通過驗證的 GCC。執行或除錯不會自動安裝工具鏈。

需要 Debug 的延伸練習請使用既有的 **Debug**、Breakpoint、Step 與 Variables；一般 Run 不會即時追蹤變數。

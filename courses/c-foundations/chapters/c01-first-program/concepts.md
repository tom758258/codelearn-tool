# 01｜第一個 C 程式

> 認識 C 程式結構。教材以繁體中文說明，C 關鍵字與程式名稱保留英文。

## 學習目標

- 辨認 `#include`、`main`、`printf` 與 `return` 的作用。
- 了解原始碼、編譯（compile）、執行（run）與輸出的差異。
- 修改字串並確認變更反映在輸出上。

## 核心觀念

程式從 `main` 函式開始執行。`#include <stdio.h>` 引入標準輸入輸出函式的宣告，這是使用 `printf` 的必要準備。`printf` 會把資料寫到標準輸出（standard output，stdout）；`\n` 是換行字元，與按 Enter 修改原始碼不是同一件事。

C 程式碼不是直接被電腦當成文字執行。使用 `Build` 時，GCC 編譯器（compiler）處理原始碼，產生可執行檔；`Run` 將執行程式並收集輸出。如果工具鏈尚未設定，仍可閱讀課文，但應先到 `Settings` 明確選擇可用 GCC 才能執行。

Markdown 裡的 C 範例僅供閱讀。請使用下方的 `Open exercise` 進入真正可以修改的 Program Block；若要回到原本練習，可依現有 `Reset exercise` 確認流程重設。

## 先預測，再實作

先猜猜 `printf` 會顯示什麼。開啟 `hello` 練習並按 `Run`，再把字串內容從 `Hello, C!` 改成 `My first program`，儲存並重新執行。比較兩次輸出。

請按本章的 **Open exercise** 開啟練習，修改已提供的檔案，再使用 **Run** 觀察 **Program output**。Markdown 中的程式碼僅供閱讀，不會直接執行。`Run` 前請依現有流程保存程式碼；若缺少 GCC 可先閱讀課文，之後到 **Settings** 選擇通過驗證的 GCC。執行或除錯不會自動安裝工具鏈。

需要 Debug 的延伸練習請使用既有的 **Debug**、Breakpoint、Step 與 Variables；一般 Run 不會即時追蹤變數。

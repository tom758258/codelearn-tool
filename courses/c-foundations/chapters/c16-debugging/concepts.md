# 16｜系統化除錯

> 找到可編譯的邏輯錯誤。教材以繁體中文說明，C 關鍵字與程式名稱保留英文。

## 學習目標

- 區分編譯錯誤（compile error）與邏輯錯誤（logic bug）。
- 使用 Breakpoint、Step Over／Into、Variables／Call Stack 追蹤加總。
- 修正起始程式中的一項刻意設計的安全邏輯錯誤。

## 核心觀念

程式可以成功編譯，卻仍輸出錯誤的結果，這叫做邏輯錯誤（logic bug）。除錯（debugging）不是盲目修改，而是先定義預期結果，再逐步觀察程式內部狀態，找出第一個偏離預期的位置。

本課提供一個可編譯且不會越界的示例。資料 `{2,4,6}` 的正確總和應是 12，但程式每輪多加了 1，因此顯示 15。請先在迴圈計算行設斷點，使用 `Step Over` 比較 `total` 與預期；`Step Into` 可用於追蹤函式呼叫。

若尚未選用有效 GDB，可先使用 `Run`、閱讀程式及人工追蹤表理解問題；真實 Breakpoint／Variables／Call Stack 需要 GDB，這些功能不會在一般 Run 自動開啟。

## 先預測，再實作

先執行並確認不正確的總和，再在 `total += values[i] + 1;` 設 Breakpoint。觀察每輪 `total` 的變化，最後刪除多餘的 `+ 1`，再執行確認結果修正。

請按本章的 **Open exercise** 開啟練習，修改已提供的檔案，再使用 **Run** 觀察 **Program output**。Markdown 中的程式碼僅供閱讀，不會直接執行。`Run` 前請依現有流程保存程式碼；若缺少 GCC 可先閱讀課文，之後到 **Settings** 選擇通過驗證的 GCC。執行或除錯不會自動安裝工具鏈。

需要 Debug 的延伸練習請使用既有的 **Debug**、Breakpoint、Step 與 Variables；一般 Run 不會即時追蹤變數。

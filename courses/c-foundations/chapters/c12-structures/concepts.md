# 12｜結構、列舉與 typedef

> 組織相關的資料。教材以繁體中文說明，C 關鍵字與程式名稱保留英文。

## 學習目標

- 使用 `struct` 將不同欄位整合成一筆資料。
- 使用 `enum` 為有限狀態命名。
- 理解 `typedef` 只是提供型別別名。

## 核心觀念

結構（structure，`struct`）允許把相關但型態不同的資料放在一起。例如商品可有名稱、數量與單價。成員使用 `item.quantity` 的形式存取。

列舉（enumeration，`enum`）可為固定選項指定有意義的名稱，例如 `AVAILABLE` 或 `SOLD_OUT`。本章的 `enum` 描述是否有庫存，而不需要使用任意魔術數字來表示狀態。

`typedef` 可以替型別建立較好讀的名稱，例如 `typedef struct {...} Product;`；它不會建立新的獨立資料庫或類別機制。先把結構與陣列結合起來練習資料整理。

## 先預測，再實作

預測商品總價，執行後將數量由 3 改成 5；觀察 `Product` 成員如何參與計算。試著把商品名稱改成另一個英文名字。

請按本章的 **Open exercise** 開啟練習，修改已提供的檔案，再使用 **Run** 觀察 **Program output**。Markdown 中的程式碼僅供閱讀，不會直接執行。`Run` 前請依現有流程保存程式碼；若缺少 GCC 可先閱讀課文，之後到 **Settings** 選擇通過驗證的 GCC。執行或除錯不會自動安裝工具鏈。

需要 Debug 的延伸練習請使用既有的 **Debug**、Breakpoint、Step 與 Variables；一般 Run 不會即時追蹤變數。

# 07｜函式與作用域

> 拆解可重用的運算。教材以繁體中文說明，C 關鍵字與程式名稱保留英文。

## 學習目標

- 辨認函式參數與回傳值。
- 將重複的運算封裝成函式。
- 使用 Step Into 和 Call Stack 觀察函式呼叫（需要 GDB）。

## 核心觀念

函式（function）把一項工作包裝成可以呼叫的單位，例如 `square(4)` 回傳 16。傳入值叫做參數（parameter）；`return` 把運算結果交回呼叫位置。

函式裡宣告的區域變數（local variable）有自己的作用域（scope），不會自動成為其他函式中的同名變數。將計算邏輯拆成簡短函式有助於閱讀和除錯。

除錯時可在 `square` 呼叫行設 Breakpoint，使用 `Step Into` 進到函式內，觀察參數和 Call Stack。一般 `Run` 只回報程式輸出，不會自動顯示區域變數。

## 先預測，再實作

先執行預設範例，接著把 `number=4` 改成 `6`。若有 GDB，從呼叫 `square(number)` 的地方 Step Into，觀察參數 `value` 如何進入函式。

請按本章的 **Open exercise** 開啟練習，修改已提供的檔案，再使用 **Run** 觀察 **Program output**。Markdown 中的程式碼僅供閱讀，不會直接執行。`Run` 前請依現有流程保存程式碼；若缺少 GCC 可先閱讀課文，之後到 **Settings** 選擇通過驗證的 GCC。執行或除錯不會自動安裝工具鏈。

需要 Debug 的延伸練習請使用既有的 **Debug**、Breakpoint、Step 與 Variables；一般 Run 不會即時追蹤變數。

# 09｜字元與字串

> 認識終止字元與有界讀取。教材以繁體中文說明，C 關鍵字與程式名稱保留英文。

## 學習目標

- 區分 `char`、`char[]` 與字串（string）。
- 理解字串以 `\0` 結束，以及 `strlen` 的意義。
- 使用有界輸入並比較字串內容。

## 核心觀念

C 字串通常是一個 `char` 陣列，以空字元（null terminator，`\0`）作為結束標記。`strlen` 回傳終止字元前的字元個數，不把終止字元本身算入長度。

`strcmp(left, right)` 比較兩個字串內容：回傳 0 表示相等。不能直接用 `==` 判斷兩個字元陣列的文字內容相等，因為陣列在表達式中的行為與字串比較函式不同。

讀取文字可使用 `fgets(buffer, sizeof buffer, stdin)`，它會限制最多讀取的字元數並保留一個位置放 `\0`。若有輸入換行，需視題目需求處理；本章先使用固定字串練習，再討論使用有界輸入的好處。

## 先預測，再實作

先執行字串 `cat` 的範例，接著把 `word` 改成 `dog`，再把 `target` 改成 `dog`，觀察字串長度及是否相等。

請按本章的 **Open exercise** 開啟練習，修改已提供的檔案，再使用 **Run** 觀察 **Program output**。Markdown 中的程式碼僅供閱讀，不會直接執行。`Run` 前請依現有流程保存程式碼；若缺少 GCC 可先閱讀課文，之後到 **Settings** 選擇通過驗證的 GCC。執行或除錯不會自動安裝工具鏈。

需要 Debug 的延伸練習請使用既有的 **Debug**、Breakpoint、Step 與 Variables；一般 Run 不會即時追蹤變數。

## 延伸練習：使用 `fgets` 有界輸入

先完成前面的固定字串比較。接著在同一個 `main.c` 中，把 `char word[] = "cat";` 改成 `char word[16] = "cat";`，保留原有的 `const char target[] = "dog";` 和 `<stdio.h>`、`<string.h>`。在兩行 `printf` **之前**加入：

```c
if (fgets(word, sizeof word, stdin) == NULL) {
    puts("No input");
    return 1;
}
word[strcspn(word, "\n")] = '\0';
```

按 **Run** 前，先在 **Standard input** 填入 `dog`（可以包含結尾換行）。預期輸出為 `Length: 3` 和 `Equal: yes`；輸入 `cat` 則為 `Length: 3`、`Equal: no`。`fgets` 最多讀入 15 個字元並保留結尾 `\0`；`strcspn` 找出換行的位置，將它改成 `\0`，避免換行影響 `strcmp`。如果沒有輸入，程式會顯示 `No input` 並回傳非零狀態。本練習只使用短於 15 個字元的英文單字，不需處理過長輸入的後續讀取。需要回到原本固定字串版本時，使用 **Reset exercise**。

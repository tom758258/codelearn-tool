# Course folders / 課程資料夾

A course is a complete folder containing `course.json`, Markdown reading sections, raster images, and C source files. Import it from **Courses → Import course**. Start with [the sample course](../examples/first-steps/course.json). The [JSON schema](course.schema.json) describes the file format; the app also checks referenced files, unique identities, compile sources, and path boundaries.

課程是完整資料夾，包含 `course.json`、Markdown 閱讀內容、點陣圖片與 C 原始碼。透過 **Courses → Import course** 匯入，可從[範例課程](../examples/first-steps/course.json)開始。[JSON Schema](course.schema.json)描述檔案格式；應用程式另會檢查引用檔案、識別碼唯一性、編譯來源與路徑邊界。

## Structure / 結構

`id` and `version` identify a course version. Importing the same pair twice is rejected. Different versions keep separate original materials and student work. Course, chapter, and block IDs use 1–48 lowercase ASCII letters, digits, hyphens, or underscores. Chapter IDs are unique within a course; block IDs are unique within a chapter. Array order defines chapter and block order. Titles are for display and can contain Unicode.

`id` 與 `version` 識別課程版本，不允許重複匯入相同組合。不同版本保留各自的原始教材與學生作業。課程、章節與區塊 ID 使用 1–48 個 ASCII 小寫字母、數字、連字號或底線。章節 ID 在課程內不可重複；區塊 ID 在同一章節內不可重複。陣列順序就是章節與區塊順序。標題用於顯示，可包含 Unicode。

- A `content` block has an `id` and `markdown`, relative to the course folder. Each block references its own `.md` file. Fenced code is for reading only.
- A `program` block has an `id`, `title`, `directory`, `files`, and `targets`. `directory` is relative to the course folder; `files` and compile paths are relative to that directory. Files must be UTF-8 `.c` or `.h` files. List every file needed by that exercise.
- `targets.primary` is one listed `.c` file. `targets.additional` lists other distinct `.c` files to link into the same program. `targets.includes` lists include directories containing exercise files; `.` means the exercise directory. Use empty arrays when no additional sources or include directories are needed.

- `content` 區塊包含 `id` 與相對於課程資料夾的 `markdown`。每個區塊引用自己的 `.md`；程式碼區塊只供閱讀。
- `program` 區塊包含 `id`、`title`、`directory`、`files` 與 `targets`。`directory` 相對於課程資料夾；`files` 與編譯路徑相對於該目錄。檔案必須是 UTF-8 `.c` 或 `.h`，並列出練習需要的所有檔案。
- `targets.primary` 指定一個已列出的 `.c`；`targets.additional` 列出其他不重複的 `.c`，共同連結成同一個程式；`targets.includes` 列出含練習檔案的 Include 目錄，`.` 表示練習目錄。不需要額外來源或目錄時使用空陣列。

Paths in `course.json` use `/` and must stay inside their specified folder. Absolute paths, `..`, links, junctions, special files, and Windows-incompatible path characters are rejected. The imported original is validated before registration; an incomplete import is never listed as a course. Failed copies remove only entries created by that copy when their identities still match. If registration fails after copying, the unregistered copy is retained and its location is reported. Existing files are never overwritten by import.

`course.json` 的路徑使用 `/`，且必須位於指定資料夾內。絕對路徑、`..`、連結、Junction、特殊檔案與 Windows 不相容的路徑字元會被拒絕。複製後會再次驗證，再加入課程清單；不完整匯入不會被列為課程。複製失敗時，只在檔案身分仍相符時清理本次建立的項目。若複製完成後登記失敗，保留未登記的副本並回報位置。匯入不覆寫既有檔案。

## Reading and exercises / 閱讀與練習

Markdown uses CommonMark through `react-markdown`. Raw HTML is ignored. Images are resolved relative to the Markdown file and must remain within the same course; PNG, JPEG, GIF, and WebP are supported. Remote images, SVG, scripts, and automatic navigation are not supported. Links display their text and destination without opening a website. Reading a course never compiles or executes its code.

Markdown 透過 `react-markdown` 使用 CommonMark，忽略原始 HTML。圖片相對於 Markdown 所在位置解析，且必須留在同一課程內；支援 PNG、JPEG、GIF 與 WebP。不支援遠端圖片、SVG、腳本或自動導覽。連結顯示文字與目的位置，不開啟網站。閱讀課程不會編譯或執行程式。

Opening an exercise for the first time copies only its listed source files into independent student work. Later opens reuse that saved work. Course exercises do not provide file creation, deletion, or renaming. **Reset exercise** restores those supplied files from the same course version after confirmation; extra files, other exercises, and other versions remain untouched. A reset failure is reported and can leave some supplied files already restored. It does not clear editor buffers on failure.

首次開啟練習時，只複製列出的來源檔案，建立獨立學生副本；之後開啟會使用已保存的作業。課程練習不提供新增、刪除或更名檔案。確認 **Reset exercise** 後，從同版本教材恢復這些檔案；保留額外檔案、其他練習與其他版本。重設失敗會明確回報，部分教材檔案可能已恢復，但不會清除尚留在編輯器中的內容。

## Ordinary projects / 一般專案

Import a complete ordinary C folder from **Projects → Import project**. It needs no CodeLearn manifest. The app edits its copied files, preserving the external folder. If there is exactly one `.c` file, it is selected as the main source; otherwise choose the main and additional C files under **Compile sources**. Include folders are optional. Choices are stored in the workspace's `Library/catalog.json`, outside project sources; no `.codelearn-project.json` is created. Makefile and CMake builds are not supported.

透過 **Projects → Import project** 匯入完整的一般 C 資料夾，不需要 CodeLearn 專用描述檔。程式只編輯複製後的檔案，保留外部資料夾。若只有一個 `.c`，會選為主要來源；否則在 **Compile sources** 選擇主要與其他 C 檔案，也可選擇 Include 目錄。設定保存在工作區的 `Library/catalog.json`，位於專案來源之外，不會建立 `.codelearn-project.json`。不支援 Makefile 或 CMake 建置。

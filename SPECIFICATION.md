# 📋 專案開發規格書：靈魂桌面．智匯看板 (WisdomBoard)
**版本：** v0.3.0
**目標：** 打造極輕量、高互動、底層嵌入式的 Windows 桌面網頁組件。

---

## 1. 系統願景 (Project Vision)
將傳統「置頂視窗」的邏輯轉向「桌面背景嵌入」。使用者無需切換視窗，即可在桌面層級與 Notion、Google Keep 等生產力工具直接互動，讓資訊像桌布一樣自然存在，卻具備動態生產力。

---

## 2. 核心功能需求 (Functional Requirements)

| 編號 | 功能名稱 | 詳細說明 | 優先級 |
| :--- | :--- | :--- | :--- |
| **F01** | **極輕量網頁渲染** | 基於 Tauri v2 與 WebView2，支援現代 Web 標準且保持極低耗電。 | 極高 |
| **F02** | **雙模式釘選 (Pinning)** | 編輯模式：`always_on_top` 置頂可操作。鎖定模式：`HWND_BOTTOM` + `WS_EX_TRANSPARENT` 置底穿透。（原 WorkerW 嵌入方案因 Windows 11 相容性已放棄） | 極高 |
| **F03** | **全交互支援** | 支援滑鼠點擊、文字輸入、捲動等操作，不只是單純顯示畫面。 | 高 |
| **F04** | **外觀客製化** | 支援無邊框 (Borderless)、自訂透明度 (Transparent) 與無任務列顯示 (Skip Taskbar)。 | 高 |
| **F05** | **開機自啟動與系統匣** | 可選配 `tauri-plugin-autostart` 達到開機即進入生產力狀態，並支援右下角的系統匣 (System Tray) 控制。 | 中 |

---

## 3. 技術架構 (Technical Architecture)

### 🛠️ 核心開發工具
* **語言：** Rust (後端邏輯與 Windows API 調用) / TypeScript + Vanilla (前端介面)
* **引擎：** Tauri v2 (基於 WebView2)
* **API 交互：** Windows Crate (`windows` crate，專注於 `User32` / `WindowsAndMessaging`)

### 🏗️ 視窗層級邏輯 (The "Magic" Logic)
面板以兩種 Z-order 狀態存在：
1. **編輯模式 (Edit)：** `always_on_top(true)` — 面板置頂，可拖移、調整大小、與內容互動。
2. **鎖定模式 (Locked)：** `SetWindowPos(HWND_BOTTOM)` + `WS_EX_TRANSPARENT` — 面板沉到底層，所有點擊穿透到下方視窗。

> 原始設計使用 `SetParent` 將面板嵌入 `WorkerW` 層（圖示下、桌布上），但因 Windows 11 相容性問題已放棄。

---

## 4. 使用者介面規範 (UI/UX Specifications)

* **無縫感：** 預設移除所有視窗控制項（標題列、縮小、關閉按鈕），並在 `tauri.conf.json` 設定 `decorations: false`。
* **無形感：** 在工具列上隱藏標籤 (`skipTaskbar: true`)，讓用戶感覺這就是 OS 的一部分。
* **雙模式切換：** 
    * *編輯模式 (Edit)：* 面板置頂，可拖移、調整大小、操作網頁內容。
    * *鎖定模式 (Locked)：* 面板置底，Click-Through 穿透，不可操作。
* **底色支援：** 面板預設不透明背景，適合釘選網頁內容。

---

## 5. 非功能性需求 (Non-functional Requirements)

* **極致低功耗 (Performance)：** 待機記憶體佔用需控制在 **40MB~80MB** 以內（對比 Electron 通常能節省數倍記憶體）。
* **穩定性：** 依賴 Rust 嚴格的記憶體安全特性，降低常駐崩潰機率。
* **安全性：** 前端 JS 僅擔任顯示層與少部分控制邏輯交互，核心 API 調用一律封裝於安全的 Rust `invoke` 指令下。

---

## 6. 開發時程預估 (Roadmap)

1. **Phase 1 (MVP)：** 建立 Tauri + TypeScript 框架，修改 `tauri.conf.json` 設定，成功顯示預設網頁。
2. **Phase 2 (Injection)：** 實作 `windows` crate API 注入邏輯，完成底層釘選 Windows 背景測試。
3. **Phase 3 (UI Fine-tuning)：** 實作系統匣 (Tray Icon) 右鍵功能選單（用於關閉程式或設定）。
4. **Phase 4 (Finalize)：** 加入開機自啟動 (`tauri-plugin-autostart`) 並建置打包安裝檔。

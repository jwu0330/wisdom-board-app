# 📐 邊框定義與螢幕模型規格書 (Border Definition & Screen Model Specification)

**版本：** v0.2.0(Phase 1+2+3+5 落地後)
**日期：** 2026-04-10
**適用專案：** WisdomBoard v0.3.0+
**前置文件：** SPECIFICATION.md, DEVELOPMENT.md

**實作狀態速查:** 見本文件 §10 後的「附錄 A」。規格書中若某章節標註為「不在 v0.4 範圍」,代表已審視過但基於「效能優先」原則刻意延後。

---

## 目錄

1. [螢幕模型定義](#1-螢幕模型定義-screen-model)
2. [座標系統定義](#2-座標系統定義-coordinate-systems)
3. [面板來源類型定義](#3-面板來源類型定義-panel-source-types)
4. [邊框定義](#4-邊框定義-border-definition)
5. [多螢幕行為規範](#5-多螢幕行為規範-multi-monitor-behavior)
6. [面板生命週期與螢幕綁定](#6-面板生命週期與螢幕綁定-panel-lifecycle--screen-binding)
7. [DPI 與縮放規範](#7-dpi-與縮放規範-dpi--scaling)
8. [邊界案例與異常處理](#8-邊界案例與異常處理-edge-cases)
9. [狀態模型擴充提案](#9-狀態模型擴充提案-state-model-extension)
10. [術語表](#10-術語表-glossary)

---

## 1. 螢幕模型定義 (Screen Model)

### 1.1 基本定義

**螢幕 (Monitor)** 是作業系統回報的一個獨立顯示裝置。每個螢幕具備以下不可變屬性（在連接期間）和可變屬性：

#### 不可變屬性（連接期間固定）

| 屬性 | 型別 | 說明 |
|------|------|------|
| `device_id` | `String` | OS 層級裝置識別碼（Windows: `\\.\DISPLAY1`）|
| `device_path` | `String` | 硬體層級唯一路徑（包含 GPU adapter + output port）|

#### 可變屬性（可能被使用者或系統改變）

| 屬性 | 型別 | 說明 |
|------|------|------|
| `physical_size` | `(u32, u32)` | 物理解析度（像素），如 `(3840, 2160)` |
| `logical_size` | `(f64, f64)` | 邏輯解析度 = `physical_size / scale_factor` |
| `position` | `(i32, i32)` | 在虛擬桌面中的物理像素偏移量（左上角）|
| `scale_factor` | `f64` | DPI 縮放倍率（1.0 = 100%, 1.5 = 150%, 2.0 = 200%）|
| `is_primary` | `bool` | 是否為主螢幕 |
| `is_available` | `bool` | 是否正在連線中 |

### 1.2 虛擬桌面 (Virtual Desktop)

Windows 將所有連接的螢幕組合成一個 **虛擬桌面座標空間 (Virtual Screen)**。

```
┌─────────────────────────────────────────────────────┐
│                  Virtual Desktop                     │
│                                                      │
│  ┌──────────────┐  ┌───────────────────────┐        │
│  │  Monitor A    │  │     Monitor B          │        │
│  │  1920×1080    │  │     3840×2160          │        │
│  │  100% DPI     │  │     150% DPI           │        │
│  │  pos:(0,0)    │  │     pos:(1920,0)       │        │
│  └──────────────┘  └───────────────────────┘        │
│                                                      │
│  ┌──────────────┐                                   │
│  │  Monitor C    │                                   │
│  │  2560×1440    │                                   │
│  │  125% DPI     │                                   │
│  │  pos:(0,1080) │                                   │
│  └──────────────┘                                   │
└─────────────────────────────────────────────────────┘
```

**關鍵規則：**

- 虛擬桌面的原點 `(0, 0)` 是**主螢幕的左上角**
- 位於主螢幕左方或上方的螢幕，其 `position` 為**負值**
- 螢幕之間可以有**間隙（dead zone）**——某些座標不屬於任何螢幕
- 虛擬桌面的邊界透過 `GetSystemMetrics(SM_XVIRTUALSCREEN)` 等取得

### 1.3 螢幕識別策略

螢幕隨時可能被拔除、新增或重新排列。需要一個穩定的識別機制：

#### 識別優先順序

```
1. device_path（硬體唯一）  →  最精確，但格式複雜
2. device_id + position      →  同一 device_id 可能因重插而換 position
3. 僅 position              →  最脆弱，螢幕重排即失效
```

#### 建議實作：複合識別碼

```
monitor_fingerprint = hash(device_path + physical_size + scale_factor)
```

**為什麼不用 HMONITOR：** `HMONITOR` 是 OS 動態分配的 handle，每次螢幕重新連接或解析度變更後可能改變，**不可持久化**。

### 1.4 螢幕狀態變化事件

| 事件 | 觸發條件 | WisdomBoard 應回應 |
|------|---------|-------------------|
| `MonitorConnect` | 螢幕插入 / 開啟 | 檢查是否有面板綁定此螢幕 → 恢復 |
| `MonitorDisconnect` | 螢幕拔除 / 關閉 | 將該螢幕上的面板**暫存** → 不銷毀 |
| `MonitorReposition` | 螢幕排列變更 | 重新計算面板的絕對座標 |
| `DpiChange` | 縮放比例變更 | 重新計算面板的邏輯 ↔ 物理座標映射 |
| `ResolutionChange` | 解析度變更 | 檢查面板是否超出邊界 → clamp |

**Windows API 對應：** 監聽 `WM_DISPLAYCHANGE` 和 `WM_DPICHANGED`。

---

## 2. 座標系統定義 (Coordinate Systems)

### 2.1 四層座標系

WisdomBoard 涉及四個座標系，必須明確區分且不得混用：

```
┌─────────────────────────────────────────────────┐
│  Layer 1: Virtual Desktop (Physical Pixels)      │
│  ─ OS 層級全域座標                                │
│  ─ GetCursorPos(), BitBlt(), SetWindowPos()      │
│  ─ 單位：物理像素                                 │
│  ─ 原點：主螢幕左上角                             │
├─────────────────────────────────────────────────┤
│  Layer 2: Monitor-Local (Physical Pixels)        │
│  ─ 單一螢幕內的座標                               │
│  ─ monitor_local = virtual - monitor.position    │
│  ─ 用途：判斷面板在哪個螢幕上                      │
├─────────────────────────────────────────────────┤
│  Layer 3: Logical Pixels (Tauri / CSS)           │
│  ─ DPI 無關的邏輯座標                             │
│  ─ logical = physical / scale_factor             │
│  ─ Tauri API、前端 CSS、持久化儲存皆用此系統        │
├─────────────────────────────────────────────────┤
│  Layer 4: Content Space (Web / App 內部)         │
│  ─ 面板內容自身的座標系                            │
│  ─ scroll offset、CSS transform 等影響           │
│  ─ 與前三層獨立                                   │
└─────────────────────────────────────────────────┘
```

### 2.2 座標轉換公式

```
# Physical → Logical（儲存用）
logical_x = (physical_x - monitor.position.x) / monitor.scale_factor + monitor.logical_position.x
logical_y = (physical_y - monitor.position.y) / monitor.scale_factor + monitor.logical_position.y

# Logical → Physical（API 呼叫用）
physical_x = (logical_x - monitor.logical_position.x) * monitor.scale_factor + monitor.position.x
physical_y = (logical_y - monitor.logical_position.y) * monitor.scale_factor + monitor.position.y
```

### 2.3 目前實作的問題

現行程式碼使用 `app.primary_monitor().scale_factor()` 作為全域 scale factor：

```rust
let scale = app.primary_monitor()
    .ok().flatten()
    .map(|m| m.scale_factor())
    .unwrap_or(1.0);
```

**問題：** 當面板位於非主螢幕且該螢幕的 DPI 與主螢幕不同時，座標轉換**必然出錯**。

**正確做法：** 根據面板所在螢幕的 scale_factor 進行轉換（見 §7）。

---

## 3. 面板來源類型定義 (Panel Source Types)

### 3.1 三種來源類型

WisdomBoard 面板根據**內容來源**分為三種類型，每種類型的邊框行為、更新策略、持久化方式皆不同：

#### Type A: 網頁面板 (Web Panel)

```
┌─ 定義 ──────────────────────────────────────────┐
│ 內容來源：URL（http/https）                       │
│ 渲染方式：Tauri WebView 直接載入                   │
│ 互動性：完整（滾動、點擊、輸入、JavaScript）        │
│ 更新方式：即時（WebView 自動渲染）                  │
│ 持久化：URL + 位置 + 尺寸                         │
│ 重啟後：重新載入 URL                               │
└──────────────────────────────────────────────────┘
```

**邊框含義：** 面板的邊框 = WebView 的可視區域邊界。內容可以在此邊界內滾動、reflow。邊框本身不裁切內容，而是定義 viewport。

**內容座標與面板座標的關係：**
- 面板座標：面板在虛擬桌面上的位置（外框）
- 內容座標：WebView 內部的 scrollX/scrollY（獨立於面板座標）
- 改變面板大小 → CSS viewport 改變 → 可能觸發 responsive reflow

#### Type B: 擷取面板 (Capture Panel) — 靜態截圖

```
┌─ 定義 ──────────────────────────────────────────┐
│ 內容來源：螢幕擷取的 BMP 圖片                      │
│ 渲染方式：<img> 標籤載入 base64 data URL           │
│ 互動性：無（純圖片）                               │
│ 更新方式：不更新（截圖時刻的快照）                   │
│ 持久化：BMP 檔案路徑 + 位置 + 尺寸                 │
│ 重啟後：需要 BMP 檔案仍存在                         │
└──────────────────────────────────────────────────┘
```

**邊框含義：** 面板邊框 = 圖片的顯示邊界。圖片與邊框的關係是 1:1 映射（截圖時決定），改變面板大小 = 縮放圖片。

**核心問題：** 截圖 BMP 存於 `%TEMP%`，OS 可能清除。需要決定：
- 是否將截圖嵌入持久化檔案（增加 config.json 大小）？
- 或接受「重啟後截圖可能遺失」？

#### Type C: App 即時面板 (App Live Panel) — **規劃中**

```
┌─ 定義 ──────────────────────────────────────────┐
│ 內容來源：其他應用程式視窗的即時畫面                  │
│ 渲染方式：DWM Thumbnail API 或定時截圖              │
│ 互動性：僅觀看（可能未來支援透傳輸入）               │
│ 更新方式：即時同步 / 定時更新                       │
│ 持久化：目標視窗識別 + 裁切區域 + 位置 + 尺寸        │
│ 重啟後：需要目標視窗仍在執行                         │
└──────────────────────────────────────────────────┘
```

**是否支援 App 面板的決策矩陣：**

| 考量 | 支援 | 不支援 |
|------|------|--------|
| 使用場景 | 監控聊天室、儀表板、終端機 | — |
| 技術複雜度 | 高（視窗識別、DWM API、權限） | 低 |
| 穩定性風險 | 高（視窗消失、HWND 改變） | 無 |
| 與 Type A 重疊 | 若目標是網頁 → 用 Type A 即可 | — |

**建議：** 分階段實作。Phase 1 僅支援 Type A + B，Phase 2 引入 Type C。

### 3.2 三種類型的邊框語義比較

| 特性 | Type A (Web) | Type B (Capture) | Type C (App Live) |
|------|-------------|-----------------|-------------------|
| 邊框代表 | viewport 邊界 | 圖片顯示邊界 | 裁切區域邊界 |
| 內容可滾動 | 是 | 否 | 取決於來源 |
| 改變面板大小 | reflow 內容 | 縮放圖片 | 縮放映射 |
| 內容比例固定 | 否（responsive） | 是（圖片比例） | 是（來源比例） |
| 邊框 = 內容邊界 | 是 | 是 | 否（可裁切子區域）|

---

## 4. 邊框定義 (Border Definition)

### 4.1 邊框的五個層次

一個 WisdomBoard 面板的「邊框」實際上是五個嵌套矩形的組合：

```
┌─ Layer 0: OS Window Frame ──────────────────────┐
│  ┌─ Layer 1: DWM Frame (Shadow / Round Corner) ┐│
│  │  ┌─ Layer 2: Tauri Decorations ───────────┐ ││
│  │  │  ┌─ Layer 3: WebView Viewport ───────┐ │ ││
│  │  │  │  ┌─ Layer 4: Content Area ──────┐ │ │ ││
│  │  │  │  │                              │ │ │ ││
│  │  │  │  │      實際可見內容              │ │ │ ││
│  │  │  │  │                              │ │ │ ││
│  │  │  │  └──────────────────────────────┘ │ │ ││
│  │  │  └────────────────────────────────────┘ │ ││
│  │  └──────────────────────────────────────────┘││
│  └──────────────────────────────────────────────┘│
└──────────────────────────────────────────────────┘
```

#### Layer 0: OS Window Frame

- **來源：** `CreateWindowEx` 時由 OS 建立
- **WisdomBoard 設定：** `decorations: false` → 此層厚度為 0
- **驗證方式：** `GetWindowRect()` vs `GetClientRect()` 差值

#### Layer 1: DWM Frame (Shadow / Round Corner)

- **來源：** Windows DWM 合成器自動加上
- **行為：** Windows 11 預設為圓角 + 投影
- **WisdomBoard 設定：** 透過 `DwmSetWindowAttribute` 移除
  - `DWMWA_WINDOW_CORNER_PREFERENCE = DWMWCP_DONOTROUND`
  - `DWMWA_USE_IMMERSIVE_DARK_MODE`（與邊框無關但影響視覺）
- **重要：** 即使設了 `decorations: false`，DWM 仍可能加上 1px 邊框或陰影
- **驗證方式：** `DwmGetWindowAttribute(DWMWA_EXTENDED_FRAME_BOUNDS)` vs `GetWindowRect()`

#### Layer 2: Tauri Decorations

- **來源：** Tauri 框架的視窗裝飾
- **WisdomBoard 設定：** `decorations: false` → 此層不存在
- **若 decorations: true：** 標題列約 32px（邏輯像素），左右下邊框各約 1-8px

#### Layer 3: WebView Viewport

- **定義：** WebView2 控制項在視窗中的實際渲染區域
- **通常等於：** Client Area（當 Layer 0-2 都被移除時）
- **例外：** 如果有自訂標題列（drag region）佔用空間

#### Layer 4: Content Area

- **定義：** 面板內容實際顯示的區域
- **計算：** Viewport - padding - toolbar - scrollbar
- **WisdomBoard 工具列：** 面板上方的模式切換 / 縮放按鈕（僅 edit 模式顯示）

### 4.2 WisdomBoard 的邊框簡化

因為 WisdomBoard 刻意移除了 Layer 0-2，實際的邊框模型簡化為：

```
面板邊框 (Panel Bounds)
  = OS Window Position + Size（設定 decorations:false 後 ≈ Client Area）
  = WebView Viewport（Layer 3）
  = Content Area + Toolbar（Layer 4 + UI chrome）
```

**但有兩個殘留邊框需要處理：**

1. **DWM 殘留邊框（Layer 1 殘留）：**
   - 即使 `decorations: false`，Windows 11 可能仍加上 1px 非客戶區邊框
   - `GetWindowRect()` 回傳的大小可能比 `GetClientRect()` 多 1-2px
   - **影響：** 截圖座標如果用 `GetWindowRect()` 會多截到邊框
   - **對策：** 一律使用 `DwmGetWindowAttribute(DWMWA_EXTENDED_FRAME_BOUNDS)` 取得真實邊界

2. **WebView2 滾動條（Layer 4 影響）：**
   - 當內容超出 viewport，會出現滾動條（預設寬度約 17px 物理像素）
   - **影響：** 改變實際 content area 寬度
   - **對策：** CSS `overflow: hidden` 或 `scrollbar-width: none`（視面板類型）

### 4.3 邊框尺寸的精確計算

```rust
/// 面板的精確邊界
struct PanelBounds {
    // 外框：OS 視窗在虛擬桌面上的位置（物理像素）
    window_rect: Rect,          // GetWindowRect()

    // 可見框：排除 DWM 陰影/圓角後的實際可見邊界（物理像素）
    frame_rect: Rect,           // DwmGetWindowAttribute(EXTENDED_FRAME_BOUNDS)

    // 客戶區：WebView 渲染區域（物理像素，相對於視窗左上角）
    client_rect: Rect,          // GetClientRect()

    // 內容區：扣除工具列後的可用內容區域（邏輯像素，相對於客戶區）
    content_rect: Rect,         // 由前端計算
}

/// 邊框厚度
struct BorderThickness {
    // window_rect 與 frame_rect 的差值（DWM 陰影）
    shadow: Insets,             // 通常 {top:0, left:7, bottom:7, right:7} on Win11

    // frame_rect 與 client_rect 的差值（非客戶區）
    frame: Insets,              // decorations:false 時通常 {0,0,0,0} 或 {0,1,1,1}

    // client_rect 與 content_rect 的差值（工具列 / padding）
    chrome: Insets,             // WisdomBoard 工具列高度
}
```

### 4.4 邊框在不同模式下的差異

| 邊框層 | Edit 模式 | Locked 模式 | Passthrough 模式 |
|--------|----------|-------------|-----------------|
| DWM Shadow | 移除 | 移除 | 移除 |
| OS Frame | 0px | 0px | 0px |
| Resize Handle | 可見（8px hit area） | 不可見 | 不可見 |
| Toolbar | 可見（面板上方） | 隱藏 | 隱藏 |
| Content Padding | 0px | 0px | 0px |

**Resize Handle 的特殊性：**
- Edit 模式下 `resizable: true`，OS 在視窗邊緣提供 8px 的拖曳調整區域
- 這個區域**不在視窗內**，而是 OS 的 hit-test 擴展
- **不影響座標計算**，但影響使用者的視覺感知

---

## 5. 多螢幕行為規範 (Multi-Monitor Behavior)

### 5.1 面板歸屬判定

每個面板歸屬於一個螢幕。判定規則：

```
1. 計算面板中心點 center = (x + width/2, y + height/2)
2. 找出 center 落在哪個螢幕的 bounds 內
3. 如果不在任何螢幕內（dead zone）→ 找最近的螢幕
4. 面板的 scale_factor 應使用**歸屬螢幕**的 scale_factor
```

### 5.2 面板跨螢幕情境

面板可能橫跨兩個不同 DPI 的螢幕：

```
┌───────────────┐┌───────────────────┐
│  Monitor A     ││    Monitor B       │
│  100% DPI      ││    150% DPI        │
│                ││                    │
│          ┌─────┼┼──────┐            │
│          │Panel│        │            │
│          │     │        │            │
│          └─────┼┼──────┘            │
│                ││                    │
└───────────────┘└───────────────────┘
```

**問題：** 面板的左半部和右半部理論上需要不同的 scale_factor。

**規範決策：** 以面板**中心點所在螢幕**的 scale_factor 為準。不嘗試拆分渲染。

**理由：**
- Windows 本身也是用這個策略（`WM_DPICHANGED` 在視窗移動跨螢幕時觸發一次）
- 拆分渲染在技術上極端複雜且無明顯收益
- 使用者可以將面板完全移到單一螢幕上來避免問題

### 5.3 螢幕斷開處理

當一個螢幕斷開時，Windows 會自動將該螢幕上的視窗移到其他螢幕。

**WisdomBoard 應做的事：**

```
螢幕斷開時:
  1. 記錄受影響面板的「原始螢幕 fingerprint」和「原始螢幕內相對座標」
  2. Windows 會自動移動視窗 → 讓它移動（不干預）
  3. 更新面板的 state 為新位置
  4. 標記面板為「已遷移 (migrated)」

同一螢幕重新連接時:
  1. 偵測到 monitor_fingerprint 匹配
  2. 將標記為「已遷移」的面板移回原位
  3. 清除遷移標記

不同螢幕連接 / 螢幕永不回來:
  1. 面板保持在遷移後的位置
  2. 使用者可手動調整
```

### 5.4 多螢幕座標持久化

**問題：** 持久化的座標應該是什麼座標系？

| 方案 | 優點 | 缺點 |
|------|------|------|
| 虛擬桌面絕對座標 | 簡單、直接 | 螢幕重排後全部錯位 |
| 螢幕相對座標 + 螢幕 ID | 螢幕重排不受影響 | 需要穩定的螢幕 ID |
| 螢幕相對座標 + 比例 | 最彈性 | 螢幕解析度變化時座標漂移 |

**建議方案：螢幕相對座標 + monitor_fingerprint**

```json
{
  "panels": [{
    "label": "panel-001",
    "monitor_fingerprint": "abc123",
    "monitor_relative_x": 0.25,
    "monitor_relative_y": 0.10,
    "x": 480.0,
    "y": 108.0,
    "width": 400.0,
    "height": 300.0
  }]
}
```

- `x`, `y`：虛擬桌面邏輯像素座標（即時使用）
- `monitor_relative_x/y`：面板左上角在螢幕內的**比例位置** (0.0 ~ 1.0)
- `monitor_fingerprint`：螢幕識別碼

**恢復流程：**
1. 先嘗試 fingerprint 匹配 → 用 `monitor_relative_x/y` 計算新絕對座標
2. 匹配失敗 → 使用 `x`, `y` 絕對座標（可能錯位，但至少有值）
3. 如果絕對座標超出所有螢幕 → clamp 到最近螢幕

---

## 6. 面板生命週期與螢幕綁定 (Panel Lifecycle & Screen Binding)

### 6.1 面板生命週期狀態機

```
[Created] ──→ [Active] ──→ [Destroyed]
                │    ↑
                ↓    │
           [Suspended]

Created:    面板視窗建立，尚未載入內容
Active:     正常運作中（edit / locked / passthrough）
Suspended:  面板的螢幕斷開，面板被遷移且標記暫停
Destroyed:  面板被使用者關閉或程式結束
```

### 6.2 各類型面板的重啟恢復行為

| 面板類型 | 目標仍可用 | 目標不可用 | 恢復行為 |
|---------|-----------|-----------|---------|
| Type A (Web) | URL 可達 | URL 不可達 | 顯示錯誤頁面，保留面板 |
| Type B (Capture) | BMP 存在 | BMP 已刪除 | 顯示「截圖已遺失」佔位 |
| Type C (App Live) | 視窗存在 | 視窗已關閉 | 顯示「來源已關閉」佔位 |

### 6.3 Type C (App Live) 的視窗識別問題

如果支援 App 面板，需要在目標應用重啟後**重新找到它**：

```
視窗識別策略（優先順序）:
1. 進程名 + 視窗類別名 + 視窗標題（模糊匹配）
2. 進程路徑（精確匹配，但需要權限）
3. HWND（僅限當次執行，不持久化）
```

**HWND 不可持久化的原因：** HWND 是 OS 動態分配的值，應用重啟後會獲得不同的 HWND。

---

## 7. DPI 與縮放規範 (DPI & Scaling)

### 7.1 DPI 感知等級

WisdomBoard 應宣告為 **Per-Monitor DPI Aware v2** (PMv2)：

```xml
<!-- tauri.conf.json 或 app.manifest -->
<dpiAwareness>PerMonitorV2</dpiAwareness>
```

**PMv2 的行為：**
- 每個視窗會收到 `WM_DPICHANGED` 當它被拖到不同 DPI 的螢幕
- OS 不會自動縮放視窗內容（不會模糊）
- 應用程式負責根據新 DPI 調整渲染

### 7.2 各層的 DPI 影響

| 層級 | DPI 影響 | 處理方式 |
|------|---------|---------|
| OS Window | 位置/大小以物理像素表示 | Tauri 自動處理 |
| Tauri | API 使用邏輯像素 | 自動乘除 scale_factor |
| WebView2 | CSS 像素 = 邏輯像素 | 自動 |
| GDI 截圖 | 物理像素 | **手動乘 scale_factor** |
| 持久化座標 | 邏輯像素 | 恢復時需知道目標螢幕 DPI |

### 7.3 截圖時的 DPI 處理

截圖涉及從 CSS 座標到 GDI 物理座標的轉換，是最容易出錯的環節：

```
使用者在 Overlay 框選 → (css_x, css_y, css_w, css_h) in CSS pixels
                          ↓
需要轉換為 → (phys_x, phys_y, phys_w, phys_h) in physical pixels
                          ↓
呼叫 BitBlt(screen_dc, phys_x, phys_y, phys_w, phys_h)
```

**正確的轉換：**

```rust
// 1. 找出 Overlay 所在的螢幕
let overlay_monitor = find_monitor_at(overlay_center_x, overlay_center_y);

// 2. 使用該螢幕的 scale_factor
let scale = overlay_monitor.scale_factor();

// 3. CSS 座標 → 虛擬桌面物理座標
let phys_x = (css_x * scale) as i32 + overlay_monitor.position.x;
let phys_y = (css_y * scale) as i32 + overlay_monitor.position.y;
let phys_w = (css_w * scale) as i32;
let phys_h = (css_h * scale) as i32;
```

**目前實作的隱患：** Overlay 是全螢幕視窗（主螢幕），如果使用者在多螢幕環境下想截非主螢幕的內容，目前的 Overlay 機制無法正確處理。

### 7.4 DPI 變更時的面板行為

當面板被拖到不同 DPI 的螢幕時：

```
WM_DPICHANGED 事件:
  1. 讀取新的 scale_factor
  2. 更新面板的 monitor_fingerprint
  3. 重新計算 monitor_relative_x/y
  4. Type B (Capture): 重新計算圖片縮放比例
  5. Type C (App Live): 重新計算裁切區域映射
```

---

## 8. 邊界案例與異常處理 (Edge Cases)

### 8.1 零面積面板

| 條件 | 處理 |
|------|------|
| `width == 0` 或 `height == 0` | 拒絕建立，回傳錯誤 |
| `width < MIN_SIZE` 或 `height < MIN_SIZE` | clamp 到 `MIN_SIZE`（建議 50 邏輯像素）|
| 框選面積太小（< 10px） | 視為取消操作 |

### 8.2 面板完全超出螢幕

```
判定條件:
  面板的任何部分都不在任何螢幕的 bounds 內

處理:
  1. 計算面板中心點
  2. 找到最近的螢幕
  3. 將面板 clamp 到該螢幕邊界內（至少露出 50px）
  4. 記錄警告 log
```

### 8.3 目標視窗消失 (Type C)

```
偵測方式:
  IsWindow(hwnd) 回傳 false
  或 GetWindowThreadProcessId(hwnd) 回傳 0

處理:
  1. 停止截圖 / DWM Thumbnail
  2. 面板顯示最後一幀畫面（凍結）+ 「來源已關閉」提示
  3. 保留面板設定（使用者可能重開目標應用）
  4. 定時輪詢（每 5 秒）嘗試用視窗識別策略重新找到目標
```

### 8.4 UAC / 管理員權限視窗

```
問題:
  非管理員進程無法擷取管理員視窗的內容
  BitBlt / PrintWindow 會回傳黑色畫面

偵測:
  擷取結果為全黑 → 可能是權限不足

處理:
  1. 提示使用者「無法擷取此視窗（需要管理員權限）」
  2. 不自動提權（安全考量）
  3. 建議使用者以管理員身份執行 WisdomBoard
```

### 8.5 高速拖曳 / 座標跳幀

```
問題:
  mousemove 事件在高速移動時可能跳過多個像素

影響:
  框選矩形在高速拖曳時不平滑（但最終座標正確）

處理:
  不需特別處理 — mousedown 和 mouseup 的座標是精確的
  框選矩形僅用於視覺回饋，不影響最終截圖座標
```

### 8.6 WisdomBoard 自身視窗被截到

```
現有處理（capture.rs L408-425）:
  open_capture_overlay() 會先隱藏所有 WisdomBoard 視窗
  等待 600ms → 截圖 → 建立 Overlay

殘留問題:
  如果使用者有其他透明 overlay 類型的應用（如 DisplayFusion）
  它們可能出現在截圖中 → 無法自動處理，屬使用者環境問題
```

### 8.7 Overlay 跨螢幕截圖

```
現狀:
  Overlay 建立為主螢幕大小的全螢幕視窗
  BitBlt 截取的是主螢幕範圍

問題:
  無法截取非主螢幕的內容

解法方向（不在本文件範圍，記錄待議）:
  A) 建立一個覆蓋所有螢幕的巨型 Overlay
  B) 在每個螢幕上各建一個 Overlay
  C) 讓使用者先選螢幕，再在該螢幕上開 Overlay
```

### 8.8 螢幕解析度 / 排列在截圖過程中變更

```
問題:
  截圖到框選之間有時間差（600ms+），期間如果螢幕狀態改變
  座標映射會完全錯誤

處理:
  1. 在框選完成後（capture_region 呼叫時）重新查詢螢幕狀態
  2. 比較截圖時與當前的螢幕 metrics
  3. 如果不一致 → 放棄本次截圖，提示使用者重試
```

### 8.9 非標準視窗邊框（目標視窗的邊框 — Type C 相關）

當 Type C (App Live) 要裁切其他應用的子區域時，需要知道目標視窗的實際內容邊界：

```
問題:
  不同應用的邊框厚度不同：
  - 標準 Win32 應用：約 8px 邊框 + 31px 標題列
  - UWP 應用：可能有不同的非客戶區
  - Electron 應用：可能 decorations:false 加上自訂標題列
  - 遊戲：可能使用獨佔全螢幕

解法:
  不嘗試「理解」目標視窗的邊框結構
  而是使用 DwmGetWindowAttribute(DWMWA_EXTENDED_FRAME_BOUNDS) 取得視覺邊界
  讓使用者在此邊界內自行框選感興趣的子區域
```

---

## 9. 狀態模型擴充提案 (State Model Extension)

基於本規格書的定義，建議對 `state.rs` 進行以下擴充：

### 9.1 新增螢幕資訊結構

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorInfo {
    /// 硬體識別指紋（持久化用）
    pub fingerprint: String,
    /// 裝置識別碼
    pub device_id: String,
    /// 物理解析度
    pub physical_width: u32,
    pub physical_height: u32,
    /// 邏輯解析度
    pub logical_width: f64,
    pub logical_height: f64,
    /// 虛擬桌面中的位置（物理像素）
    pub position_x: i32,
    pub position_y: i32,
    /// DPI 縮放倍率
    pub scale_factor: f64,
    /// 是否為主螢幕
    pub is_primary: bool,
}
```

### 9.2 擴充 PanelConfig

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelConfig {
    pub label: String,
    pub panel_type: PanelType,          // Url, Capture, AppLive（新增）
    pub url: Option<String>,

    // === 位置（邏輯像素，虛擬桌面絕對座標）===
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,

    // === 螢幕綁定（新增）===
    pub monitor_fingerprint: Option<String>,
    /// 面板左上角在歸屬螢幕內的比例位置 (0.0~1.0)
    pub monitor_relative_x: Option<f64>,
    pub monitor_relative_y: Option<f64>,

    pub mode: String,
    pub zoom: f64,

    // === Type C: App Live 相關（新增）===
    /// 目標視窗識別（進程名 + 視窗類別 + 標題模式）
    pub target_window_match: Option<WindowMatch>,
    /// 目標視窗內的裁切區域（比例，0.0~1.0）
    pub source_crop: Option<[f64; 4]>,   // [x%, y%, w%, h%]

    // === 遷移狀態（新增）===
    /// 面板是否因螢幕斷開而被遷移
    #[serde(default)]
    pub is_migrated: bool,

    // === 既有保留欄位 ===
    #[serde(skip)]
    pub target_hwnd: Option<isize>,
    #[serde(skip)]
    pub source_rect: Option<[i32; 4]>,
    #[serde(skip_serializing, default)]
    pub screenshot_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowMatch {
    pub process_name: Option<String>,      // e.g. "chrome.exe"
    pub window_class: Option<String>,      // e.g. "Chrome_WidgetWin_1"
    pub title_pattern: Option<String>,     // regex pattern
}
```

### 9.3 擴充 PanelType

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PanelType {
    Url,        // Type A: 網頁面板
    Capture,    // Type B: 靜態截圖面板
    AppLive,    // Type C: App 即時面板（Phase 2）
}
```

---

## 10. 術語表 (Glossary)

| 術語 | 定義 |
|------|------|
| **虛擬桌面 (Virtual Desktop / Virtual Screen)** | 所有螢幕組合成的統一座標空間 |
| **物理像素 (Physical Pixel)** | 螢幕上的實際像素，與硬體解析度對應 |
| **邏輯像素 (Logical Pixel / CSS Pixel)** | DPI 無關的抽象像素，`物理 = 邏輯 × scale_factor` |
| **Scale Factor** | DPI 縮放倍率。100% = 1.0, 150% = 1.5, 200% = 2.0 |
| **Monitor Fingerprint** | 螢幕硬體的穩定識別碼，用於跨重啟追蹤螢幕 |
| **Panel Bounds** | 面板在虛擬桌面上的位置與大小 |
| **Content Area** | 面板內部扣除 UI chrome（工具列等）後的實際內容區域 |
| **DWM Frame** | Windows DWM 合成器添加的視覺邊框（陰影、圓角） |
| **Client Area** | 視窗中排除標題列和邊框後的可用渲染區域 |
| **Hit Test** | OS 判斷滑鼠點擊落在視窗哪個區域的機制 |
| **Clamp** | 將超出範圍的值強制限制到有效範圍內 |
| **Migrate** | 面板因螢幕斷開而被自動移到其他螢幕的行為 |

---

## 附錄 A: 現有程式碼與本規格的差距

**最後更新:** 2026-04-10(v0.4 Phase 1+2+3+5 落地後)

| 項目 | 現狀(落地後) | 狀態 |
|------|------|--------|
| DPI 處理 | `monitor::scale_for_physical_point` 使用面板所屬螢幕 scale | **已實作 (Phase 1)** |
| 單一事實來源 | `monitor.rs` 為唯一的螢幕資訊入口;其他模組禁止直接呼叫 `primary_monitor()` / `SM_CXSCREEN` | **已實作 (Phase 1)** |
| 螢幕識別 | `monitor::compute_fingerprint`(name + size + scale hash) | **已實作 (Phase 2)** |
| 座標持久化 | `PanelConfig` 新增 `monitor_fingerprint` + `monitor_relative_x/y` + `is_migrated`,全部 `#[serde(default)]` 向下相容 | **已實作 (Phase 2)** |
| Moved 事件跨螢幕追蹤 | `Moved` handler 自動用歸屬螢幕 scale 轉換並更新 fingerprint/relative | **已實作 (Phase 2)** |
| fingerprint restore | `restore_panels` 先查 fingerprint → 若命中則用 relative 重算絕對座標 → fallback 到原絕對座標 | **已實作 (Phase 3)** |
| 面板超出螢幕 clamp | `monitor::clamp_rect_to_monitors`,若面板與所有螢幕無交集則移到最近螢幕 | **已實作 (Phase 3)** |
| 面板最小尺寸 | `panel::MIN_PANEL_SIZE = 50.0`,前後端對齊 | **已實作 (Phase 5)** |
| 螢幕斷開處理 | 被動:OS 自動搬移 → `Moved` 事件自動更新 fingerprint | **被動處理(刻意不做主動 watcher)** |
| Overlay 多螢幕 | 僅主螢幕 | **不在 v0.4 範圍(§8.7)** |
| DWM 殘留邊框 | 未處理 | **不在 v0.4 範圍(目前無消費者)** |
| 截圖快照一致性 | 未處理 | **不在 v0.4 範圍(< 2 秒邊界情境)** |
| Type C (App Live) | 僅保留 enum 空位 | **不在 v0.4 範圍** |

### 設計決策:為何不實作主動螢幕狀態 watcher

規格書 §1.4 建議監聽 `WM_DISPLAYCHANGE` / `WM_DPICHANGED` 並主動協調所有面板。經評估後**刻意不實作**,原因:

1. **效能優先**:會引入一個常駐 message-only window 執行緒、~1MB 記憶體、~200 LOC 複雜度。雖然閒置 CPU 為 0,但複雜度對專案「效能最優先」的原則不利。
2. **使用頻率極低**:絕大多數桌面使用情境下,螢幕配置一次設定後不改動。熱插拔發生時使用者通常也會重啟應用。
3. **被動處理已覆蓋大部分正確性**:
   - **螢幕拔掉**:Windows 自動將視窗搬到剩下的螢幕 → Tauri `Moved` 事件觸發 → Phase 2 的 handler 自動更新 `monitor_fingerprint` 與 `monitor_relative_x/y`。這等同於「軟遷移」。
   - **螢幕插回來**:面板留在新位置。使用者若想回到原位,重啟 app 即可 — `restore_panels` 會用原 fingerprint 找回螢幕(Phase 3 已支援)。
   - **DPI 變更**:`Moved` 事件同時觸發,自動用新螢幕 scale 重新計算邏輯座標。
4. **簡單勝過聰明**:被動處理 = 0 新執行緒、0 新 LOC、0 新狀態機。符合最小驚訝原則。

若未來真的需要主動 watcher(例如「螢幕斷開時面板淡出動畫」這類即時 UX 需求),再另開計畫實作,屆時 `PanelConfig.is_migrated` 欄位已就位可直接使用。

---

## 附錄 B: 測試驗證矩陣

以下組合應在實作時逐一驗證：

| # | 座標系 | DPI | 螢幕數 | 面板類型 | 預期行為 |
|---|--------|-----|--------|---------|---------|
| 1 | 主螢幕 | 100% | 單螢幕 | Type A | 基準：座標精確 |
| 2 | 主螢幕 | 150% | 單螢幕 | Type A | 座標正確縮放 |
| 3 | 主螢幕 | 100% | 單螢幕 | Type B | 截圖像素精確 |
| 4 | 主螢幕 | 150% | 單螢幕 | Type B | 截圖 DPI 正確 |
| 5 | 副螢幕 | 與主相同 | 雙螢幕 | Type A | 座標偏移正確 |
| 6 | 副螢幕 | 與主不同 | 雙螢幕 | Type A | scale_factor 正確切換 |
| 7 | 跨螢幕 | 混合 DPI | 雙螢幕 | Type A | 使用中心點螢幕的 DPI |
| 8 | 螢幕斷開 | — | 雙→單 | Type A | 面板遷移 + 記錄 |
| 9 | 螢幕重連 | — | 單→雙 | Type A | 面板恢復原位 |
| 10 | 主螢幕 | 100% | 單螢幕 | Type B (BMP遺失) | 顯示佔位提示 |

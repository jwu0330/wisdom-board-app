//! 螢幕資訊與座標轉換 — 唯一的事實來源
//!
//! 對應 BORDER_SPECIFICATION.md:
//! - §1 螢幕模型定義
//! - §2 座標系統定義
//! - §5.1 面板歸屬判定
//! - §7 DPI 與縮放規範
//!
//! 其他模組一律透過本模組取得螢幕資訊與做座標轉換,
//! 禁止直接呼叫 `app.primary_monitor()` 或 `GetSystemMetrics(SM_CXSCREEN)`。

// 本模組預先暴露了 Phase 3/4/5 將使用的 API(find_by_fingerprint、
// virtual_desktop_physical_bounds、logical_size 等)。為避免 Phase 1/2 編譯警告
// 干擾後續重構,統一使用模組層級 allow;等各 API 被對應 Phase 真正呼叫後,
// 可再移除此 allow。
#![allow(dead_code)]

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use tauri::AppHandle;

/// 螢幕快照資訊(對應規格書 §1.1)
///
/// 所有座標都明確標註是 physical 還是 logical,避免混淆。
#[derive(Debug, Clone)]
pub struct MonitorInfo {
    /// 硬體指紋(對應規格書 §1.3,用於持久化後重新匹配螢幕)
    pub fingerprint: String,
    /// 作業系統回報的螢幕名稱(Windows: `\\.\DISPLAY1`)
    pub name: String,
    /// 虛擬桌面中的位置(物理像素,左上角)
    pub physical_position: (i32, i32),
    /// 物理解析度(物理像素)
    pub physical_size: (u32, u32),
    /// DPI 縮放倍率(1.0 = 100%, 1.5 = 150%, …)
    pub scale_factor: f64,
    /// 是否為主螢幕
    pub is_primary: bool,
}

impl MonitorInfo {
    /// 邏輯解析度(以自身 scale 為基準的邏輯像素大小)
    pub fn logical_size(&self) -> (f64, f64) {
        (
            self.physical_size.0 as f64 / self.scale_factor,
            self.physical_size.1 as f64 / self.scale_factor,
        )
    }

    /// 某個物理虛擬桌面座標是否落在此螢幕內
    pub fn contains_physical(&self, px: i32, py: i32) -> bool {
        let (x, y) = self.physical_position;
        let (w, h) = self.physical_size;
        px >= x && px < x + w as i32 && py >= y && py < y + h as i32
    }
}

/// 列舉目前所有連線中的螢幕
///
/// 對應規格書 §1.2 虛擬桌面。主螢幕判定:Tauri 的 `primary_monitor()` 為準。
pub fn enumerate(app: &AppHandle) -> Vec<MonitorInfo> {
    let primary_name: Option<String> = app
        .primary_monitor()
        .ok()
        .flatten()
        .and_then(|m| m.name().cloned());

    let monitors = match app.available_monitors() {
        Ok(list) => list,
        Err(e) => {
            eprintln!("[WisdomBoard] 列舉螢幕失敗: {e}");
            return Vec::new();
        }
    };

    monitors
        .into_iter()
        .map(|m| {
            let name = m.name().cloned().unwrap_or_default();
            let pos = m.position();
            let size = m.size();
            let scale = m.scale_factor();
            let is_primary = primary_name.as_deref() == Some(name.as_str());
            MonitorInfo {
                fingerprint: compute_fingerprint(&name, (size.width, size.height), scale),
                name,
                physical_position: (pos.x, pos.y),
                physical_size: (size.width, size.height),
                scale_factor: scale,
                is_primary,
            }
        })
        .collect()
}

/// 主螢幕 scale factor(fallback 情境用)
pub fn primary_scale_factor(app: &AppHandle) -> f64 {
    app.primary_monitor()
        .ok()
        .flatten()
        .map(|m| m.scale_factor())
        .unwrap_or(1.0)
}

/// 根據物理虛擬桌面座標找到所屬螢幕
///
/// 若該座標不在任何螢幕內(位於 dead zone),回退到主螢幕(規格書 §5.1)。
pub fn find_by_physical_point<'a>(
    monitors: &'a [MonitorInfo],
    px: i32,
    py: i32,
) -> Option<&'a MonitorInfo> {
    monitors
        .iter()
        .find(|m| m.contains_physical(px, py))
        .or_else(|| monitors.iter().find(|m| m.is_primary))
        .or_else(|| monitors.first())
}

/// 根據「以主螢幕 scale 為基準的邏輯座標」找所屬螢幕
///
/// 這是為了相容現行 PanelConfig 的 x,y 儲存語意(`pos.x / primary_scale`)。
pub fn find_by_primary_logical_point<'a>(
    monitors: &'a [MonitorInfo],
    logical_x: f64,
    logical_y: f64,
) -> Option<&'a MonitorInfo> {
    let primary_scale = monitors
        .iter()
        .find(|m| m.is_primary)
        .map(|m| m.scale_factor)
        .unwrap_or(1.0);
    let px = (logical_x * primary_scale) as i32;
    let py = (logical_y * primary_scale) as i32;
    find_by_physical_point(monitors, px, py)
}

/// 以面板中心點判定歸屬螢幕(規格書 §5.1)
///
/// 輸入為「以主螢幕 scale 為基準的邏輯座標」(與現有 PanelConfig x,y 同義)。
pub fn find_by_panel_rect<'a>(
    monitors: &'a [MonitorInfo],
    logical_x: f64,
    logical_y: f64,
    logical_w: f64,
    logical_h: f64,
) -> Option<&'a MonitorInfo> {
    let cx = logical_x + logical_w / 2.0;
    let cy = logical_y + logical_h / 2.0;
    find_by_primary_logical_point(monitors, cx, cy)
}

/// 根據指紋找螢幕
pub fn find_by_fingerprint<'a>(
    monitors: &'a [MonitorInfo],
    fingerprint: &str,
) -> Option<&'a MonitorInfo> {
    monitors.iter().find(|m| m.fingerprint == fingerprint)
}

/// 虛擬桌面的物理邊界:(left, top, right, bottom)
///
/// 對應 `SM_XVIRTUALSCREEN` 等(規格書 §1.2)。
pub fn virtual_desktop_physical_bounds() -> (i32, i32, i32, i32) {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    };
    unsafe {
        let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let h = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        (x, y, x + w, y + h)
    }
}

/// 計算螢幕指紋(規格書 §1.3)
///
/// 用 name + 物理尺寸 + scale 的 hash,避免直接用不穩定的 HMONITOR。
fn compute_fingerprint(name: &str, size: (u32, u32), scale: f64) -> String {
    let mut hasher = DefaultHasher::new();
    name.hash(&mut hasher);
    size.0.hash(&mut hasher);
    size.1.hash(&mut hasher);
    scale.to_bits().hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// 將面板的邏輯位置轉為歸屬螢幕內的相對比例(0.0 ~ 1.0),
/// 用於跨重啟的螢幕綁定儲存(規格書 §5.4)。
///
/// 回傳 `(relative_x, relative_y)`。
pub fn compute_relative_position(
    monitor: &MonitorInfo,
    panel_logical_x: f64,
    panel_logical_y: f64,
    primary_scale: f64,
) -> (f64, f64) {
    // 面板座標現在存的是「主螢幕 scale 為基準的邏輯虛擬桌面座標」
    // 換算回物理虛擬桌面座標,再減去螢幕物理位置,除以螢幕物理大小。
    let panel_phys_x = panel_logical_x * primary_scale;
    let panel_phys_y = panel_logical_y * primary_scale;
    let rel_x = (panel_phys_x - monitor.physical_position.0 as f64)
        / monitor.physical_size.0 as f64;
    let rel_y = (panel_phys_y - monitor.physical_position.1 as f64)
        / monitor.physical_size.1 as f64;
    (rel_x, rel_y)
}

/// 從螢幕指紋 + 相對比例還原絕對邏輯座標(規格書 §5.4 恢復流程)
///
/// 回傳以主螢幕 scale 為基準的邏輯虛擬桌面座標(與 `PanelConfig.x/y` 同義)。
pub fn resolve_from_relative(
    monitor: &MonitorInfo,
    relative_x: f64,
    relative_y: f64,
    primary_scale: f64,
) -> (f64, f64) {
    let phys_x = monitor.physical_position.0 as f64
        + relative_x * monitor.physical_size.0 as f64;
    let phys_y = monitor.physical_position.1 as f64
        + relative_y * monitor.physical_size.1 as f64;
    (phys_x / primary_scale, phys_y / primary_scale)
}

/// 將面板 clamp 到至少與一個螢幕有交集的位置(規格書 §8.2)
///
/// 輸入/回傳:以主螢幕 scale 為基準的邏輯座標。
///
/// 規則:
/// 1. 若面板與任一螢幕有交集 → 原封不動回傳,`was_clamped = false`
/// 2. 否則 → 移動到最近螢幕的左上角 + 邊距,`was_clamped = true`
///
/// 效能:O(n) 其中 n = 螢幕數(通常 1~3),只在 restore 時呼叫。
pub fn clamp_rect_to_monitors(
    monitors: &[MonitorInfo],
    primary_scale: f64,
    logical_x: f64,
    logical_y: f64,
    logical_w: f64,
    logical_h: f64,
) -> (f64, f64, bool) {
    if monitors.is_empty() {
        return (logical_x, logical_y, false);
    }

    // 轉為物理座標判定
    let px = (logical_x * primary_scale) as i32;
    let py = (logical_y * primary_scale) as i32;
    let pw = (logical_w * primary_scale).max(1.0) as i32;
    let ph = (logical_h * primary_scale).max(1.0) as i32;

    // 是否與任一螢幕有矩形交集
    let intersects = monitors.iter().any(|m| {
        let (mx, my) = m.physical_position;
        let (mw, mh) = m.physical_size;
        let mx2 = mx + mw as i32;
        let my2 = my + mh as i32;
        px < mx2 && (px + pw) > mx && py < my2 && (py + ph) > my
    });

    if intersects {
        return (logical_x, logical_y, false);
    }

    // 面板完全在所有螢幕外 → 找中心點最近的螢幕
    let cx = px + pw / 2;
    let cy = py + ph / 2;
    let nearest = monitors
        .iter()
        .min_by_key(|m| {
            let (mx, my) = m.physical_position;
            let (mw, mh) = m.physical_size;
            let mcx = mx + (mw / 2) as i32;
            let mcy = my + (mh / 2) as i32;
            let dx = (mcx - cx) as i64;
            let dy = (mcy - cy) as i64;
            dx * dx + dy * dy
        })
        .unwrap(); // 上面已檢查非空

    // 放到最近螢幕的左上角 + 40px 物理邊距(避免貼齊邊角)
    let margin: i32 = 40;
    let new_px = nearest.physical_position.0 + margin;
    let new_py = nearest.physical_position.1 + margin;
    let new_x = new_px as f64 / primary_scale;
    let new_y = new_py as f64 / primary_scale;
    (new_x, new_y, true)
}

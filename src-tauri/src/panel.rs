use crate::state::{ManagedState, PanelConfig, PanelType};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use tauri::{AppHandle, Emitter, Manager};
use windows::Win32::Foundation::HWND;
use std::time::{SystemTime, UNIX_EPOCH};

/// 移除 Windows 11 視窗圓角 + 陰影 + 邊框
pub fn set_square_corners(win: &tauri::WebviewWindow) {
    if let Ok(raw) = win.hwnd() {
        let hwnd = HWND(raw.0 as isize);
        unsafe {
            use windows::Win32::Graphics::Dwm::DwmSetWindowAttribute;

            // 1. 直角
            let preference: u32 = 1; // DWMWCP_DONOTROUND
            let _ = DwmSetWindowAttribute(
                hwnd,
                windows::Win32::Graphics::Dwm::DWMWA_WINDOW_CORNER_PREFERENCE,
                &preference as *const u32 as *const _,
                4,
            );

            // 2. 關閉 DWM 非客戶區渲染（消除陰影）
            let policy: u32 = 1; // DWMNCRP_DISABLED
            let _ = DwmSetWindowAttribute(
                hwnd,
                windows::Win32::Graphics::Dwm::DWMWINDOWATTRIBUTE(2), // DWMWA_NCRENDERING_POLICY
                &policy as *const u32 as *const _,
                4,
            );

            // 3. 設定邊框顏色為 DWMWA_BORDER_COLOR = none（-2 = DWMWA_COLOR_NONE）
            let no_border: u32 = 0xFFFFFFFE; // DWMWA_COLOR_NONE
            let _ = DwmSetWindowAttribute(
                hwnd,
                windows::Win32::Graphics::Dwm::DWMWINDOWATTRIBUTE(34), // DWMWA_BORDER_COLOR
                &no_border as *const u32 as *const _,
                4,
            );

            // 4. 關閉標題列顏色（消除任何殘留的邊框色）
            let _ = DwmSetWindowAttribute(
                hwnd,
                windows::Win32::Graphics::Dwm::DWMWINDOWATTRIBUTE(35), // DWMWA_CAPTION_COLOR
                &no_border as *const u32 as *const _,
                4,
            );
        }
    }
}

/// 鎖定視窗：置底 + WS_EX_TRANSPARENT + WS_EX_LAYERED（不用 WS_DISABLED 以免 WebView2 停止渲染）
fn lock_window(hwnd: HWND) {
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::*;
        // 加上 WS_EX_TRANSPARENT + WS_EX_LAYERED 讓滑鼠穿透（WebView2 繼續渲染）
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        SetWindowLongW(hwnd, GWL_EXSTYLE,
            (ex | WS_EX_TRANSPARENT.0 | WS_EX_LAYERED.0) as i32);
        // 設定 layered window 為完全不透明（只為了讓 WS_EX_TRANSPARENT 生效）
        use windows::Win32::UI::WindowsAndMessaging::{SetLayeredWindowAttributes, LWA_ALPHA};
        let _ = SetLayeredWindowAttributes(hwnd, windows::Win32::Foundation::COLORREF(0), 255, LWA_ALPHA);
        // 置底
        let _ = SetWindowPos(hwnd, HWND_BOTTOM, 0, 0, 0, 0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

/// 解鎖視窗：移除 WS_EX_TRANSPARENT + WS_EX_LAYERED + WS_EX_NOACTIVATE
fn unlock_window(hwnd: HWND) {
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::*;
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        SetWindowLongW(hwnd, GWL_EXSTYLE,
            (ex & !(WS_EX_TRANSPARENT.0 | WS_EX_LAYERED.0 | WS_EX_NOACTIVATE.0)) as i32);
    }
}

static PANEL_COUNT: AtomicU32 = AtomicU32::new(0);
/// debounce：記錄上次 Moved/Resized 觸發 auto_save 的時間戳（毫秒）
static LAST_SAVE_TICK: AtomicU64 = AtomicU64::new(0);
/// Moved/Resized 事件在停止 500ms 後才寫入持久化
const SAVE_DEBOUNCE_MS: u64 = 500;
/// 恢復面板時延遲套用 mode（等 WebView2 初次渲染完成才套用 locked/passthrough 的視窗屬性）
const RESTORE_MODE_DELAY_MS: u64 = 300;

/// 通用 debounce：只在停止觸發 SAVE_DEBOUNCE_MS 後才執行 auto_save
fn debounced_auto_save(app: &AppHandle) {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    LAST_SAVE_TICK.store(now_ms, Ordering::Relaxed);
    let app_clone = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(SAVE_DEBOUNCE_MS));
        let saved_at = LAST_SAVE_TICK.load(Ordering::Relaxed);
        if saved_at == now_ms {
            crate::persistence::auto_save(&app_clone);
        }
    });
}

pub fn next_panel_id() -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let seq = PANEL_COUNT.fetch_add(1, Ordering::SeqCst);
    format!("panel-{}-{}", ts, seq)
}

/// 找到一個物理虛擬桌面座標所屬螢幕的 scale factor
///
/// 這取代了原本寫死主螢幕 scale 的 `get_scale()`。若該座標不在任何螢幕內
/// (dead zone),退回主螢幕 scale。對應規格書 §2.3、§7.2。
fn scale_for_physical_point(app: &AppHandle, px: i32, py: i32) -> f64 {
    let monitors = crate::monitor::enumerate(app);
    crate::monitor::find_by_physical_point(&monitors, px, py)
        .map(|m| m.scale_factor)
        .unwrap_or_else(|| crate::monitor::primary_scale_factor(app))
}

/// 建構 `PanelConfig` 的統一入口,自動填寫螢幕綁定欄位(規格書 §5.4、§9.2)。
///
/// 這是所有「建立面板」路徑(create_panel / create_url_panel* / capture_region)
/// 的唯一構造函式,確保 fingerprint / relative 欄位不會遺漏。
#[allow(clippy::too_many_arguments)]
pub(crate) fn make_panel_config(
    app: &AppHandle,
    label: String,
    panel_type: PanelType,
    url: Option<String>,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    mode: &str,
    screenshot_path: Option<String>,
) -> PanelConfig {
    let monitors = crate::monitor::enumerate(app);
    let primary_scale = crate::monitor::primary_scale_factor(app);
    let binding = crate::monitor::find_by_panel_rect(&monitors, x, y, width, height)
        .map(|m| {
            let (rx, ry) =
                crate::monitor::compute_relative_position(m, x, y, primary_scale);
            (m.fingerprint.clone(), rx, ry)
        });
    PanelConfig {
        label,
        panel_type,
        url,
        x,
        y,
        width,
        height,
        mode: mode.into(),
        zoom: 1.0,
        screenshot_path,
        monitor_fingerprint: binding.as_ref().map(|b| b.0.clone()),
        monitor_relative_x: binding.as_ref().map(|b| b.1),
        monitor_relative_y: binding.as_ref().map(|b| b.2),
        is_migrated: false,
    }
}

pub fn handle_panel_event(app: &AppHandle, label: &str, event: &tauri::WindowEvent) {
    match event {
        tauri::WindowEvent::Destroyed => {
            let screenshot_path: Option<String> = {
                let state = app.state::<ManagedState>();
                let result = if let Ok(mut guard) = state.lock() {
                    guard.panels.remove(label).and_then(|p| p.screenshot_path)
                } else {
                    None
                };
                result
            };
            // 清理截圖暫存檔
            if let Some(path) = screenshot_path {
                let _ = std::fs::remove_file(&path);
            }
            crate::persistence::auto_save(app);
            let _ = app.emit("panel-closed", label);
        }
        tauri::WindowEvent::Moved(pos) => {
            // 使用面板所屬螢幕的 scale 進行轉換,而非寫死主螢幕 scale。
            // 規格書 §2.3:多螢幕異 DPI 時必須用歸屬螢幕 scale。
            let scale = scale_for_physical_point(app, pos.x, pos.y);
            let new_x = pos.x as f64 / scale;
            let new_y = pos.y as f64 / scale;

            // 先在鎖外算出新的螢幕綁定資訊(fingerprint + relative),
            // 避免持有鎖時再呼叫 monitor::enumerate。
            let (width, height) = {
                let state = app.state::<ManagedState>();
                state
                    .lock()
                    .ok()
                    .and_then(|g| g.panels.get(label).map(|p| (p.width, p.height)))
                    .unwrap_or((0.0, 0.0))
            };
            let monitors = crate::monitor::enumerate(app);
            let primary_scale = crate::monitor::primary_scale_factor(app);
            let binding = crate::monitor::find_by_panel_rect(
                &monitors, new_x, new_y, width, height,
            )
            .map(|owning| {
                let (rx, ry) = crate::monitor::compute_relative_position(
                    owning, new_x, new_y, primary_scale,
                );
                (owning.fingerprint.clone(), rx, ry)
            });

            let state = app.state::<ManagedState>();
            if let Ok(mut guard) = state.lock() {
                if let Some(p) = guard.panels.get_mut(label) {
                    p.x = new_x;
                    p.y = new_y;
                    if let Some((fp, rx, ry)) = binding {
                        p.monitor_fingerprint = Some(fp);
                        p.monitor_relative_x = Some(rx);
                        p.monitor_relative_y = Some(ry);
                    }
                }
            };
            debounced_auto_save(app);
        }
        tauri::WindowEvent::Resized(size) => {
            // Resized 事件不帶位置,透過視窗查詢當前物理位置決定所屬螢幕
            let scale = app
                .get_webview_window(label)
                .and_then(|w| w.outer_position().ok())
                .map(|pos| scale_for_physical_point(app, pos.x, pos.y))
                .unwrap_or_else(|| crate::monitor::primary_scale_factor(app));
            let state = app.state::<ManagedState>();
            if let Ok(mut guard) = state.lock() {
                if let Some(p) = guard.panels.get_mut(label) {
                    p.width = size.width as f64 / scale;
                    p.height = size.height as f64 / scale;
                }
            };
            debounced_auto_save(app);
        }
        _ => {}
    }
}

/// 關閉所有面板與 overlay，並儲存狀態
#[tauri::command]
pub fn close_all_panels(app: AppHandle) -> Result<(), String> {
    let labels: Vec<String> = app.webview_windows()
        .into_iter()
        .filter(|(label, _)| label.starts_with("panel-") || label == "overlay")
        .map(|(label, _)| label)
        .collect();

    // 先清空 state，清理截圖暫存檔，避免 Destroyed handler 重複操作
    {
        let state = app.state::<ManagedState>();
        if let Ok(mut guard) = state.lock() {
            for p in guard.panels.values() {
                if let Some(ref path) = p.screenshot_path {
                    let _ = std::fs::remove_file(path);
                }
            }
            guard.panels.clear();
        };
    }

    for label in &labels {
        if let Some(win) = app.get_webview_window(label) {
            let _ = win.close();
            println!("[WisdomBoard] 關閉視窗: {}", label);
        }
    }

    crate::persistence::auto_save(&app);
    println!("[WisdomBoard] 全部面板已關閉");
    Ok(())
}

#[tauri::command]
pub fn list_panels(app: AppHandle) -> Vec<serde_json::Value> {
    let windows: Vec<(String, String)> = app.webview_windows()
        .into_iter()
        .filter(|(label, _)| label.starts_with("panel-"))
        .map(|(label, win)| {
            let title = win.title().unwrap_or_default();
            (label, title)
        })
        .collect();

    // 一次鎖定讀取所有面板資料，確保一致性
    let state = app.state::<ManagedState>();
    let guard = state.lock().ok();

    windows.into_iter()
        .map(|(label, title)| {
            let (panel_type, url, mode, zoom, screenshot) = guard.as_ref()
                .and_then(|g| {
                    g.panels.get(&label).map(|p| (
                        if p.panel_type == PanelType::Url { "url" } else { "capture" },
                        p.url.clone(),
                        p.mode.clone(),
                        p.zoom,
                        p.screenshot_path.clone(),
                    ))
                })
                .unwrap_or(("capture", None, "locked".to_string(), 1.0, None));
            serde_json::json!({
                "label": label,
                "title": title,
                "type": panel_type,
                "url": url,
                "mode": mode,
                "zoom": zoom,
                "screenshot_path": screenshot,
            })
        })
        .collect()
}

#[tauri::command]
pub fn create_url_panel(app: AppHandle, url: String) -> Result<String, String> {
    let label = next_panel_id();

    let _parsed: url::Url = url
        .parse()
        .map_err(|e: url::ParseError| format!("網址格式錯誤: {e}"))?;

    let config = make_panel_config(
        &app,
        label.clone(),
        PanelType::Url,
        Some(url.clone()),
        0.0, 0.0, 800.0, 600.0,
        "locked",
        None,
    );
    {
        let state = app.state::<ManagedState>();
        let mut guard = state.lock().map_err(|e| format!("state lock 失敗: {e}"))?;
        guard.panels.insert(label.clone(), config);
        drop(guard);
    }

    build_url_panel_async(app, label.clone(), url.clone(), 0.0, 0.0, 800.0, 600.0, false);

    println!("[WisdomBoard] URL 面板 {} 建立中: {}", label, url);
    Ok(label)
}

/// 在指定位置和大小建立 URL 面板（從 overlay 框選呼叫）
#[tauri::command]
pub fn create_url_panel_at(
    app: AppHandle, url: String,
    x: f64, y: f64, width: f64, height: f64,
) -> Result<String, String> {
    let label = next_panel_id();

    url.parse::<url::Url>()
        .map_err(|e: url::ParseError| format!("網址格式錯誤: {e}"))?;

    let config = make_panel_config(
        &app,
        label.clone(),
        PanelType::Url,
        Some(url.clone()),
        x, y, width, height,
        "locked",
        None,
    );
    {
        let state = app.state::<ManagedState>();
        let mut guard = state.lock().map_err(|e| format!("state lock 失敗: {e}"))?;
        guard.panels.insert(label.clone(), config);
        drop(guard);
    }

    build_url_panel_async(app, label.clone(), url.clone(), x, y, width, height, true);

    Ok(label)
}

/// URL 面板建立的共用邏輯（在獨立執行緒中執行，避免阻塞 command handler）
fn build_url_panel_async(
    app: AppHandle, label: String, url: String,
    x: f64, y: f64, width: f64, height: f64,
    with_position: bool,
) {
    std::thread::spawn(move || {
        let parsed_url: url::Url = match url.parse() {
            Ok(u) => u,
            Err(e) => { println!("[WisdomBoard] URL parse error: {e}"); return; }
        };
        let webview_url = tauri::WebviewUrl::External(parsed_url);
        let mut builder = tauri::WebviewWindowBuilder::new(&app, &label, webview_url)
            .title(format!("WisdomBoard - {}", url))
            .inner_size(width, height)
            .decorations(false)
            .always_on_top(false)
            .skip_taskbar(true)
            .transparent(false)
            .on_navigation(|nav_url| {
                let u = nav_url.as_str();
                !u.starts_with("about:") && !u.starts_with("chrome:")
            });

        if with_position {
            builder = builder.position(x, y);
        }

        match builder.build() {
            Ok(win) => {
                set_square_corners(&win);
                let _ = set_panel_mode(app.clone(), label.clone(), "locked".into());
                let app_handle = app.clone();
                let panel_label = label.clone();
                win.on_window_event(move |event| {
                    handle_panel_event(&app_handle, &panel_label, event);
                });
                crate::persistence::auto_save(&app);
                println!("[WisdomBoard] URL 面板 {} 已建立: {} @ ({},{}) {}x{}",
                    label, url, x, y, width, height);
                let _ = app.emit("panel-created", serde_json::json!({
                    "label": &label, "type": "url", "url": &url, "mode": "locked"
                }));
            }
            Err(e) => {
                println!("[WisdomBoard] URL 面板 build() FAILED: {e}");
                let state = app.state::<ManagedState>();
                if let Ok(mut guard) = state.lock() {
                    guard.panels.remove(&label);
                }
                let _ = app.emit("panel-create-failed", &label);
            }
        }
    });
}

#[tauri::command]
pub fn set_panel_mode(app: AppHandle, label: String, mode: String) -> Result<(), String> {
    let window = app
        .get_webview_window(&label)
        .ok_or_else(|| format!("找不到面板: {}", label))?;
    let _ = app.emit_to(&label, "mode-changed", &mode);

    // 判斷面板類型
    let is_url = {
        let state = app.state::<ManagedState>();
        state.lock().ok()
            .and_then(|g| g.panels.get(&label).map(|p| p.panel_type == PanelType::Url))
            .unwrap_or(false)
    };

    // 三態模式：
    // "edit"        → 置頂 + 可拖移調整
    // "passthrough" → 置底 + 可操作面板內容（WS_EX_NOACTIVATE 防止跳到前面）
    // "locked"      → 置底 + 滑鼠穿透（WS_EX_TRANSPARENT + WS_EX_LAYERED，不用 WS_DISABLED 以免 WebView2 停止渲染）
    match mode.as_str() {
        "edit" => {
            if let Ok(raw) = window.hwnd() {
                let hwnd = HWND(raw.0 as isize);
                unlock_window(hwnd);
            }
            let _ = window.set_always_on_top(true);
            let _ = window.set_resizable(true);
            let _ = window.show();
            let _ = window.set_focus();
            if is_url {
                let _ = window.eval(
                    "(() => {\
                       var d = document.getElementById('wb-drag-overlay');\
                       if (!d) {\
                         d = document.createElement('div');\
                         d.id = 'wb-drag-overlay';\
                         d.style.cssText = 'position:fixed;inset:0;z-index:99999;cursor:move;-webkit-app-region:drag;background:rgba(137,180,250,0.08);';\
                         document.documentElement.appendChild(d);\
                       } else { d.style.display = 'block'; }\
                       if (window.__wb_orig_requestFullscreen) {\
                         Element.prototype.requestFullscreen = window.__wb_orig_requestFullscreen;\
                         delete window.__wb_orig_requestFullscreen;\
                       }\
                       if (window.__wb_orig_exitFullscreen) {\
                         document.exitFullscreen = window.__wb_orig_exitFullscreen;\
                         delete window.__wb_orig_exitFullscreen;\
                       }\
                     })();"
                );
            }
        }
        "passthrough" => {
            // 穿透：可操作內容，但強制在底層，不跳到前面
            if let Ok(raw) = window.hwnd() {
                let hwnd = HWND(raw.0 as isize);
                unsafe {
                    use windows::Win32::UI::WindowsAndMessaging::*;
                    // 移除 WS_EX_TRANSPARENT（接收滑鼠）但保留 WS_EX_LAYERED
                    let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
                    let new_ex = (ex & !WS_EX_TRANSPARENT.0) | WS_EX_LAYERED.0;
                    SetWindowLongW(hwnd, GWL_EXSTYLE, new_ex as i32);
                    // alpha=255 完全不透明
                    let _ = SetLayeredWindowAttributes(hwnd, windows::Win32::Foundation::COLORREF(0), 255,
                        windows::Win32::UI::WindowsAndMessaging::LWA_ALPHA);
                    // 加上 WS_EX_NOACTIVATE 防止點擊時激活到前面
                    let ex2 = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
                    SetWindowLongW(hwnd, GWL_EXSTYLE, (ex2 | WS_EX_NOACTIVATE.0) as i32);
                    // 置底
                    let _ = SetWindowPos(hwnd, HWND_BOTTOM, 0, 0, 0, 0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                }
            }
            let _ = window.set_always_on_top(false);
            let _ = window.set_resizable(false);
            if is_url {
                let _ = window.eval(
                    "var d=document.getElementById('wb-drag-overlay'); if(d) d.style.display='none';\
                     if (!window.__wb_orig_requestFullscreen) window.__wb_orig_requestFullscreen = Element.prototype.requestFullscreen;\
                     if (!window.__wb_orig_exitFullscreen) window.__wb_orig_exitFullscreen = document.exitFullscreen;\
                     Element.prototype.requestFullscreen=function(){return Promise.resolve();};\
                     document.exitFullscreen=function(){return Promise.resolve();};"
                );
            }
        }
        _ => {
            // locked（都關）：置底 + 完全禁止互動
            let _ = window.set_always_on_top(false);
            let _ = window.set_resizable(false);
            if let Ok(raw) = window.hwnd() {
                let hwnd = HWND(raw.0 as isize);
                lock_window(hwnd);
            }
            if is_url {
                let _ = window.eval(
                    "var d=document.getElementById('wb-drag-overlay'); if(d) d.style.display='none';"
                );
            }
        }
    }

    {
        let state = app.state::<ManagedState>();
        if let Ok(mut guard) = state.lock() {
            if let Some(p) = guard.panels.get_mut(&label) {
                p.mode = mode.clone();
            }
        };
    }
    crate::persistence::auto_save(&app);
    Ok(())
}

#[tauri::command]
pub fn set_panel_zoom(app: AppHandle, label: String, zoom: f64) -> Result<(), String> {
    if zoom < 0.1 || zoom > 5.0 {
        return Err(format!("zoom 值超出範圍: {}", zoom));
    }
    let window = app
        .get_webview_window(&label)
        .ok_or_else(|| format!("找不到面板: {}", label))?;
    let js = format!(
        "document.documentElement.style.transform = 'scale({z})'; \
         document.documentElement.style.transformOrigin = 'top left'; \
         document.documentElement.style.width = '{w}%'; \
         document.documentElement.style.height = '{h}%'; \
         document.documentElement.style.overflow = 'hidden';",
        z = zoom,
        w = 100.0 / zoom,
        h = 100.0 / zoom,
    );
    let _ = window.eval(&js);

    {
        let state = app.state::<ManagedState>();
        if let Ok(mut guard) = state.lock() {
            if let Some(p) = guard.panels.get_mut(&label) {
                p.zoom = zoom;
            }
        };
    }
    crate::persistence::auto_save(&app);
    Ok(())
}

#[tauri::command]
pub fn close_panel(app: AppHandle, label: String) -> Result<(), String> {
    let window = app
        .get_webview_window(&label)
        .ok_or_else(|| format!("找不到面板: {}", label))?;
    window.close().map_err(|e| format!("{e}"))
}

#[tauri::command]
pub fn set_mode(app: AppHandle, mode: String) -> Result<(), String> {
    let labels: Vec<String> = app.webview_windows()
        .into_keys()
        .filter(|l| l.starts_with("panel-"))
        .collect();
    let mut errors = Vec::new();
    for label in labels {
        if let Err(e) = set_panel_mode(app.clone(), label.clone(), mode.clone()) {
            errors.push(format!("{}: {}", label, e));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// 從持久化設定恢復面板（啟動時呼叫）
pub fn restore_panels(app: &AppHandle, configs: Vec<PanelConfig>) {
    for config in configs {
        let result = match config.panel_type {
            PanelType::Url => {
                if let Some(ref url) = config.url {
                    restore_url_panel(app, &config, url)
                } else {
                    continue;
                }
            }
            PanelType::Capture => restore_capture_panel(app, &config),
        };

        match result {
            Ok(label) => {
                println!("[WisdomBoard] 已恢復面板: {}", label);
                let a = app.clone();
                let l = label;
                let m = config.mode.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(RESTORE_MODE_DELAY_MS));
                    let _ = set_panel_mode(a, l, m);
                });
            }
            Err(e) => eprintln!("[WisdomBoard] 恢復面板失敗: {e}"),
        }
    }
}

fn restore_url_panel(app: &AppHandle, config: &PanelConfig, url: &str) -> Result<String, String> {
    let label = config.label.clone();
    let parsed_url: url::Url = url.parse().map_err(|e: url::ParseError| format!("URL 解析失敗: {e}"))?;
    let webview_url = tauri::WebviewUrl::External(parsed_url);
    let is_edit = config.mode != "locked";

    let builder = tauri::WebviewWindowBuilder::new(app, &label, webview_url)
        .title(format!("WisdomBoard - {}", url))
        .inner_size(config.width, config.height)
        .position(config.x, config.y)
        .decorations(false)
        .always_on_top(is_edit)
        .skip_taskbar(true)
        .transparent(false)
        .on_navigation(|nav_url| {
            let u = nav_url.as_str();
            !u.starts_with("about:") && !u.starts_with("chrome:")
        });

    let win = builder.build().map_err(|e| format!("{e}"))?;
    set_square_corners(&win);

    {
        let state = app.state::<ManagedState>();
        if let Ok(mut guard) = state.lock() {
            guard.panels.insert(label.clone(), PanelConfig {
                label: label.clone(),
                url: Some(url.to_string()),
                ..config.clone()
            });
        };
    }

    {
        let app_handle = app.clone();
        let panel_label = label.clone();
        win.on_window_event(move |event| {
            handle_panel_event(&app_handle, &panel_label, event);
        });
    }

    Ok(label)
}

fn restore_capture_panel(app: &AppHandle, config: &PanelConfig) -> Result<String, String> {
    let label = config.label.clone();
    let url = tauri::WebviewUrl::App("src/panel.html".into());
    let is_edit = config.mode != "locked";

    let builder = tauri::WebviewWindowBuilder::new(app, &label, url)
        .title("WisdomBoard Capture".to_string())
        .inner_size(config.width, config.height)
        .position(config.x, config.y)
        .decorations(false)
        .always_on_top(is_edit)
        .skip_taskbar(true)
        .transparent(false);

    let win = builder.build().map_err(|e| format!("{e}"))?;
    set_square_corners(&win);

    {
        let state = app.state::<ManagedState>();
        if let Ok(mut guard) = state.lock() {
            guard.panels.insert(label.clone(), PanelConfig {
                label: label.clone(),
                ..config.clone()
            });
        };
    }

    {
        let app_handle = app.clone();
        let panel_label = label.clone();
        win.on_window_event(move |event| {
            handle_panel_event(&app_handle, &panel_label, event);
        });
    }

    Ok(label)
}

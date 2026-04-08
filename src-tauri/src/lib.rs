// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

use tauri::{Manager, Runtime, WebviewWindow};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, SendMessageW, SetParent};

pub fn pin_to_desktop<R: Runtime>(window: &WebviewWindow<R>) {
    let hwnd = window.hwnd().unwrap().0 as isize;
    let hwnd = HWND(hwnd as *mut _);

    unsafe {
        // 1. 找到 Progman
        let progman = FindWindowW(None, None);
        
        // 2. 發送 0x052C 訊息，觸發 WorkerW 生成
        SendMessageW(progman, 0x052C, WPARAM(0), LPARAM(0));

        // 3. 為了簡化初期開發，我們先將父層設為 progman (未來可改進為尋找正確的 WorkerW)
        SetParent(hwnd, progman); 
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // 在啟動時將主視窗掛載到桌面底層
            let main_window = app.get_webview_window("main").unwrap();
            pin_to_desktop(&main_window);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![greet])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

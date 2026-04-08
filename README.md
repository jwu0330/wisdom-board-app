# WisdomBoard 靈魂桌面．智匯看板

Windows 桌面應用程式，基於 Tauri v2 + TypeScript。

核心功能：
- 將網頁內容以面板形式釘選於桌面最前層
- 框選螢幕區域建立擷取面板
- 系統匣常駐，開機自啟動
- 全域快捷鍵 Ctrl+Alt+S 開啟設定

## 技術架構

| 層級 | 技術 |
|------|------|
| 前端 | TypeScript + Vite + HTML/CSS |
| 後端 | Rust + Tauri v2 |
| 系統 API | windows crate 0.52 |
| 外掛 | tauri-plugin-autostart |

## 開發

本專案透過 GitHub Actions 建置，詳見 [DEVELOPMENT.md](DEVELOPMENT.md)。

```bash
# 推送觸發 CI/CD
git push origin main

# 下載建置產物
gh run download -n WisdomBoard-Portable
```

## 授權

私人專案

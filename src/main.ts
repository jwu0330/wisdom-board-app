// WisdomBoard 前端進入點
// 目前作為桌面看板的載入控制器

const TARGET_URL = "https://github.com/users/jwu0330/projects/2/views/3";

function init() {
  const status = document.getElementById("status");

  // 直接透過 WebView 導航到目標 URL（繞過 iframe X-Frame-Options 限制）
  // 如果需要嵌入外部網頁，使用 window.location 而非 iframe
  window.location.href = TARGET_URL;

  // 若 3 秒後仍在此頁面（表示導航失敗），顯示錯誤訊息
  setTimeout(() => {
    if (status) {
      status.innerHTML = `
        <div style="text-align:center;">
          <p style="font-size:20px; margin-bottom:12px;">WisdomBoard</p>
          <p style="font-size:13px; opacity:0.6;">無法載入目標頁面</p>
          <p style="font-size:12px; opacity:0.4; margin-top:8px;">${TARGET_URL}</p>
        </div>
      `;
    }
  }, 3000);
}

// DOM 載入完成後初始化
if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", init);
} else {
  init();
}

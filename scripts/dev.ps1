# Talos Dev Server Launcher
# Legacy local launcher. The retired remote-sync bridge has no LAN, SSRF, or
# insecure-TLS switches; local recovery remains explicitly opt-in through its
# separate restricted script.

Write-Host @"
  ╔══════════════════════════════════════╗
  ║   Talos Rental — Dev Launcher     ║
  ╚══════════════════════════════════════╝
"@

Write-Host "正在启动 Node.js 后端 (端口 8080)..." -ForegroundColor Cyan
Write-Host "按 Ctrl+C 停止" -ForegroundColor DarkGray
Write-Host ""

node server.js

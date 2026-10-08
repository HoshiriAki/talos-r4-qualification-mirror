@echo off
echo ========================================
echo 应用租户数据库迁移
echo ========================================
echo.

echo 正在备份数据库...
copy ..\rental.db ..\rental.db.backup
if %ERRORLEVEL% NEQ 0 (
    echo [✗] 备份失败
    exit /b 1
)
echo [✓] 数据库已备份到 rental.db.backup
echo.

echo 正在应用迁移...
echo.

rem 使用 cargo 的迁移系统
cd ..
cargo run --bin migrate-db
if %ERRORLEVEL% NEQ 0 (
    echo [!] cargo migrate 失败，尝试手动修复...

    rem 检查是否有 sqlite3
    where sqlite3 >nul 2>&1
    if %ERRORLEVEL% EQU 0 (
        echo [✓] 找到 sqlite3，应用手动修复...
        sqlite3 rental.db < backend\fix_tenant_columns.sql
        echo [✓] 手动迁移完成
    ) else (
        echo.
        echo [!] 未找到 sqlite3 命令
        echo.
        echo 请手动运行：
        echo   1. 安装 SQLite: https://www.sqlite.org/download.html
        echo   2. 运行: sqlite3 rental.db
        echo   3. 执行: .read backend/fix_tenant_columns.sql
        echo   4. 退出: .quit
        echo.
        echo 或者删除旧数据库，让系统重新创建：
        echo   del rental.db
        echo   start.cmd
        echo.
        pause
        exit /b 1
    )
)

cd backend
echo.
echo ========================================
echo 迁移完成！
echo ========================================
echo.
echo 现在可以启动系统：
echo   cd ..
echo   start.cmd
echo.
pause

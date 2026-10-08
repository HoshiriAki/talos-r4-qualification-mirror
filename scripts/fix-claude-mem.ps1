# fix-claude-mem.ps1
# One-click repair for claude-mem "Cannot find module 'zod/v3'" error.
# Runs `bun install --production` in every claude-mem version directory that
# is missing node_modules, or where package.json is newer than node_modules.
#
# Usage:
#   pwsh -File scripts/fix-claude-mem.ps1          # manual
#   pwsh -NoProfile -NonInteractive -File scripts/fix-claude-mem.ps1  # hook-safe

$ErrorActionPreference = "Stop"
$PLUGIN_CACHE = Join-Path $env:USERPROFILE ".claude\plugins\cache\thedotmack\claude-mem"

if (-not (Test-Path $PLUGIN_CACHE)) {
    Write-Host "[fix-claude-mem] claude-mem plugin cache not found at $PLUGIN_CACHE"
    exit 0
}

# Resolve bun — try .cmd first (what 'where bun' returns), then .ps1 fallback
$bunExe = $null
$bunCandidates = @(
    (Join-Path $env:APPDATA "npm\bun.cmd"),
    (Join-Path $env:APPDATA "npm\bun.ps1"),
    (Join-Path $env:USERPROFILE ".bun\bin\bun.exe")
)
foreach ($candidate in $bunCandidates) {
    if (Test-Path $candidate) {
        $bunExe = $candidate
        break
    }
}

if (-not $bunExe) {
    # Last resort: try PATH
    try { $bunExe = (Get-Command bun -ErrorAction Stop).Source } catch {}
}

if (-not $bunExe) {
    Write-Host "[fix-claude-mem] bun not found — install bun first: https://bun.sh"
    exit 1
}

Write-Host "[fix-claude-mem] using bun: $bunExe"

$fixed = 0
$skipped = 0
$failed = 0

Get-ChildItem $PLUGIN_CACHE -Directory -ErrorAction SilentlyContinue | ForEach-Object {
    $versionDir = $_.FullName
    $pkgJson = Join-Path $versionDir "package.json"
    $nodeModules = Join-Path $versionDir "node_modules"

    if (-not (Test-Path $pkgJson)) {
        Write-Host "[fix-claude-mem] SKIP $($_.Name): no package.json"
        return
    }

    $needInstall = $false
    if (-not (Test-Path $nodeModules)) {
        $needInstall = $true
        Write-Host "[fix-claude-mem] FIX  $($_.Name): node_modules missing"
    }
    else {
        # Check if package.json is newer → likely a partial/incomplete install
        $pkgTime = (Get-Item $pkgJson).LastWriteTime
        $nmTime = (Get-Item $nodeModules).LastWriteTime
        if ($pkgTime -gt $nmTime) {
            $needInstall = $true
            Write-Host "[fix-claude-mem] FIX  $($_.Name): package.json newer than node_modules"
        }
    }

    if (-not $needInstall) {
        $skipped++
        return
    }

    try {
        $result = & $bunExe install --production 2>&1
        if ($LASTEXITCODE -eq 0) {
            $fixed++
            Write-Host "[fix-claude-mem] DONE $($_.Name)"
        }
        else {
            $failed++
            Write-Host "[fix-claude-mem] FAIL $($_.Name): $result"
        }
    }
    catch {
        $failed++
        Write-Host "[fix-claude-mem] FAIL $($_.Name): $_"
    }
}

Write-Host "[fix-claude-mem] Summary: $fixed fixed, $skipped skipped, $failed failed"
if ($failed -gt 0) { exit 1 } else { exit 0 }

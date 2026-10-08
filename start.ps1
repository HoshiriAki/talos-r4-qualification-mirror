<#
.SYNOPSIS
  TALOS Runtime Supervisor PowerShell 入口。

.DESCRIPTION
  与 start.cmd / pnpm start 使用同一 scripts/start-supervisor.mjs。
  支持 run、debug、trace、doctor、diagnose、dump 和 clean-logs 模式，
  并可把端口参数传递给原有 start.js 进程控制内核。

.EXAMPLE
  .\start.ps1
  .\start.ps1 debug
  .\start.ps1 doctor
  .\start.ps1 debug -BackendPort 3001 -FrontendPort 5174
  .\start.ps1 diagnose --no-archive
#>

[CmdletBinding()]
param(
  [ValidateSet('run', 'debug', 'trace', 'doctor', 'diagnose', 'dump', 'clean-logs')]
  [string]$Mode = 'run',

  [ValidateRange(0, 65535)]
  [int]$BackendPort = 0,

  [ValidateRange(0, 65535)]
  [int]$FrontendPort = 0,

  [Parameter(ValueFromRemainingArguments = $true)]
  [string[]]$SupervisorArgs
)

$supervisor = Join-Path $PSScriptRoot 'scripts\start-supervisor.mjs'

if (-not (Test-Path -LiteralPath $supervisor -PathType Leaf)) {
  Write-Error "Missing runtime supervisor: $supervisor"
  exit 66
}

$node = Get-Command node -ErrorAction SilentlyContinue
if (-not $node) {
  Write-Error 'Node.js was not found in PATH. Install Node.js 22 and reopen the terminal.'
  exit 69
}

$nodeArgs = @($supervisor)

if ($Mode -ne 'run') {
  $nodeArgs += $Mode
}

if ($SupervisorArgs) {
  $nodeArgs += $SupervisorArgs
}

if ($BackendPort -gt 0 -or $FrontendPort -gt 0) {
  $nodeArgs += '--'
  if ($BackendPort -gt 0) {
    $nodeArgs += '--port'
    $nodeArgs += [string]$BackendPort
  }
  if ($FrontendPort -gt 0) {
    $nodeArgs += '--frontend-port'
    $nodeArgs += [string]$FrontendPort
  }
}

& $node.Source @nodeArgs
exit $LASTEXITCODE

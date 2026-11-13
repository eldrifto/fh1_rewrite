# Overnight driver for docs/NIGHT_PLAN.md -- stock OpenCode, no plugins.
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File tools\night_run.ps1
#
# Loops `opencode run` against the `night` agent, one plan step per turn, and stops when
# data/work/night/REPORT.md exists (the plan's own completion marker), when the time budget
# runs out, when data/work/night/STOP exists, or when the plan stops changing.
#
# Nothing here judges the work; the loop only checks files. That is deliberate: the model
# must not be the one deciding whether it is finished.

param(
  [double]$Hours = 6,
  [int]$MaxIters = 80,
  [string]$Root = 'D:\Projects\slop\fh1_rewrite',
  [string]$Agent = 'night',
  [string]$Session = 'ses_night_plan',
  [string]$Model = 'opencode-go/deepseek-v4.1-flash'
)

$ErrorActionPreference = 'Stop'

# --- find the CLI (stock desktop install keeps it outside PATH) -------------------------
$candidates = @(
  'opencode',
  'C:\Users\tippiebekfast\AppData\Roaming\ai.opencode.desktop\cli\2.2.6-x64\opencode-cli.exe',
  'C:\Users\tippiebekfast\AppData\Local\Programs\ai.opencode.desktop\resources\opencode-cli.exe'
)
$cli = $null
foreach ($c in $candidates) {
  if (Test-Path $c) { $cli = $c; break }
  $cmd = Get-Command $c -ErrorAction SilentlyContinue
  if ($cmd) { $cli = $cmd.Source; break }
}
if (-not $cli) {
  throw 'opencode CLI not found. Add the desktop CLI folder to PATH, or edit $candidates in this script.'
}

# the session's working directory must be the project, so the .opencode agent is discovered
Set-Location $Root

$night  = Join-Path $Root 'data\work\night'
$report = Join-Path $night 'REPORT.md'
$stop   = Join-Path $night 'STOP'
$log    = Join-Path $night 'cli.log'
$plan   = Join-Path $Root 'docs\NIGHT_PLAN.md'

New-Item -ItemType Directory -Force -Path $night | Out-Null

if (-not (Test-Path $plan)) { throw "plan missing: $plan" }

# --- completion check happens before anything else -------------------------------------
if (Test-Path $report) {
  Write-Host 'PLAN ALREADY COMPLETE: data/work/night/REPORT.md exists. Nothing to do.'
  exit 0
}
if (Test-Path $stop) {
  Write-Host 'STOP FILE PRESENT. Remove data/work/night/STOP to run again.'
  exit 0
}

if (-not (Test-Path (Join-Path $night '00_started.txt'))) {
  Get-Date -Format 'o' | Set-Content -Path (Join-Path $night '00_started.txt') -Encoding utf8
}

$deadline = (Get-Date).AddHours($Hours)
$message = @'
Continue docs/NIGHT_PLAN.md. Do exactly one step: the lowest-numbered step whose status is
not DONE/BLOCKED/SKIPPED. Follow it exactly as written, write its evidence file, update its
status row in the plan, then end this turn. If every step is DONE/BLOCKED/SKIPPED, write
data/work/night/REPORT.md per section N8 and end the turn.
'@

Write-Host "cli     : $cli"
Write-Host "agent   : $Agent"
Write-Host "session : $Session"
Write-Host "model   : $Model"
Write-Host "budget  : $Hours h (until $($deadline.ToString('HH:mm')))"
Write-Host "log     : $log"
Write-Host "stop    : create $stop"
Write-Host ''

# --- one turn --------------------------------------------------------------------------
# Native stderr must not be fatal: Windows PowerShell 5.1 turns a native command's stderr into
# an ErrorRecord, and EAP=Stop would abort the whole night on a progress line. Merge it, log
# it, and judge the turn by the exit code instead.
function Invoke-Turn {
  $argv = @('run', '--agent', $Agent, '--auto', '--archived', '--session', $Session)
  if ($Model) { $argv += @('--model', $Model) }
  $argv += $message
  $ErrorActionPreference = 'Continue'
  try {
    & $cli @argv 2>&1 | ForEach-Object { "$_" } | Tee-Object -FilePath $log -Append
  } finally {
    $ErrorActionPreference = 'Stop'
  }
  return $LASTEXITCODE
}

$iter  = 0
$stale = 0
$fails = 0
$code  = 1

while ((Get-Date) -lt $deadline -and $iter -lt $MaxIters) {
  if (Test-Path $report) { $code = 0; break }
  if (Test-Path $stop)   { Write-Host 'STOP file present, halting.'; break }

  $iter++
  $before = (Get-Item $plan).LastWriteTime
  Write-Host ("[iter {0}] {1}" -f $iter, (Get-Date -Format 'HH:mm:ss'))

  $exit = Invoke-Turn
  if ($exit -eq 0) {
    $fails = 0
  } else {
    $fails++
    Write-Host ("  turn exited {0} (consecutive failures: {1})" -f $exit, $fails)
    if ($fails -ge 3) { Write-Host 'Three consecutive CLI failures. Halting.'; break }
  }

  $after = (Get-Item $plan).LastWriteTime
  if ($after -eq $before) { $stale++ } else { $stale = 0 }
  if ($stale -ge 3) {
    Write-Host 'No plan change for 3 turns (agent idle or stuck). Halting.'
    break
  }
  Start-Sleep -Seconds 5
}

if (Test-Path $report) {
  Write-Host ''
  Write-Host '=== PLAN COMPLETE ==='
  Write-Host "report: $report"
  exit 0
}

if ((Get-Date) -ge $deadline) { Write-Host 'Time budget reached.' }
elseif ($iter -ge $MaxIters) { Write-Host 'Iteration cap reached.' }
else { Write-Host 'Halted without the completion marker.' }

Write-Host 'Check the plan status table and the log above before re-running.'
exit 1

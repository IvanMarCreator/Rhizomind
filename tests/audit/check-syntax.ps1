# Синтаксис помощника сетевого аудита в Windows PowerShell 5.1 и PSScriptAnalyzer
# (задача 3.3б, решение владельца 102). Запускается в powershell.exe (5.1).
$ErrorActionPreference = 'Stop'
try { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8 } catch { $null = $_ }

$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$files = @(
  (Join-Path $root 'docs\security\network-audit.ps1'),
  (Join-Path $PSScriptRoot 'parse.tests.ps1'),
  (Join-Path $PSScriptRoot 'run-unattended.ps1'),
  (Join-Path $PSScriptRoot 'check-syntax.ps1')
)
$failed = 0
Write-Host "PowerShell $($PSVersionTable.PSVersion) ($($PSVersionTable.PSEdition))"

foreach ($f in $files) {
  $name = $f.Substring($root.Length + 1)
  # Windows PowerShell 5.1 читает файл без BOM в кодировке ANSI — кириллица
  # превратилась бы в мусор. Все файлы — UTF-8 с BOM.
  $head = [System.IO.File]::ReadAllBytes($f)[0..2]
  if (-not ($head[0] -eq 0xEF -and $head[1] -eq 0xBB -and $head[2] -eq 0xBF)) {
    Write-Host "FAIL $name : нет BOM UTF-8"
    $failed++
  }
  $tokens = $null; $errors = $null
  [void][System.Management.Automation.Language.Parser]::ParseFile($f, [ref]$tokens, [ref]$errors)
  if ($errors.Count -gt 0) {
    foreach ($e in $errors) { Write-Host ("FAIL {0}:{1}:{2} {3}" -f $name, $e.Extent.StartLineNumber, $e.Extent.StartColumnNumber, $e.Message) }
    $failed++
  } else {
    Write-Host "OK   синтаксис PowerShell $($PSVersionTable.PSVersion.Major).$($PSVersionTable.PSVersion.Minor): $name — ошибок разбора 0"
  }
}

$pssa = Get-Module -ListAvailable -Name PSScriptAnalyzer | Sort-Object Version -Descending | Select-Object -First 1
if (-not $pssa) {
  Write-Host 'SKIP PSScriptAnalyzer: модуля нет в образе раннера'
} else {
  Import-Module $pssa.Path
  Write-Host "PSScriptAnalyzer $($pssa.Version)"
  foreach ($f in $files) {
    $name = $f.Substring($root.Length + 1)
    $all = @(Invoke-ScriptAnalyzer -Path $f -Severity @('Error', 'Warning'))
    $errs = @($all | Where-Object { $_.Severity -eq 'Error' })
    foreach ($w in @($all | Where-Object { $_.Severity -ne 'Error' })) {
      Write-Host ("  предупреждение {0}:{1} {2}: {3}" -f $name, $w.Line, $w.RuleName, $w.Message)
    }
    if ($errs.Count -gt 0) {
      foreach ($e in $errs) { Write-Host ("FAIL {0}:{1} {2}: {3}" -f $name, $e.Line, $e.RuleName, $e.Message) }
      $failed++
    } else {
      Write-Host "OK   PSScriptAnalyzer (правила уровня Error): $name — ошибок 0, предупреждений $($all.Count)"
    }
  }
}

if ($failed -gt 0) { Write-Host "ИТОГ: ошибок $failed"; exit 1 }
Write-Host 'ИТОГ: синтаксис и анализатор — без ошибок'
exit 0

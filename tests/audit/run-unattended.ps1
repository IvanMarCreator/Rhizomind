# Неинтерактивный прогон помощника сетевого аудита на настоящей выпускной
# сборке (задача 3.3б, решение владельца 102): -Unattended -NoProcmon, короткий
# простой, без сценариев. Скрипт должен дойти до results.md с ожидаемыми
# разделами и флагами движка. Плюс два коротких отказа: нет пути к программе,
# папка результатов в перемещаемом профиле.
# Запуск: powershell -NoProfile -ExecutionPolicy Bypass -File tests/audit/run-unattended.ps1 -AppExe <rhizomind.exe>
param(
  [Parameter(Mandatory = $true)] [string] $AppExe,
  [int] $IdleSeconds = 30
)
$ErrorActionPreference = 'Stop'
try { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8 } catch { $null = $_ }

$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$auditScript = Join-Path $root 'docs\security\network-audit.ps1'
$base = $env:RUNNER_TEMP
if (-not $base) { $base = $env:TEMP }
$script:Failures = 0
function Check([string] $name, [bool] $condition, [string] $detail = '') {
  if ($condition) { Write-Host "OK   $name" } else { Write-Host "FAIL $name $detail"; $script:Failures++ }
}

function Invoke-Audit([string[]] $arguments) {
  Write-Host "> powershell.exe -File network-audit.ps1 $($arguments -join ' ')"
  & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $auditScript @arguments 2>&1 | ForEach-Object { Write-Host "  | $_" }
  return $LASTEXITCODE
}

# --- Отказы -------------------------------------------------------------------

$code = Invoke-Audit @('-Unattended', '-NoProcmon', '-OutDir', (Join-Path $base 'audit-noexe'))
Check 'без -AppExe в автоматическом режиме — понятный выход с кодом 2, без вопросов' ($code -eq 2) "code=$code"

$code = Invoke-Audit @('-Unattended', '-NoProcmon', '-AppExe', $AppExe, '-OutDir', (Join-Path $env:APPDATA 'rhizomind-audit-test'))
Check 'папка результатов в перемещаемом профиле — отказ с кодом 2' ($code -eq 2) "code=$code"
Check 'папка результатов в перемещаемом профиле не создана' (-not (Test-Path -LiteralPath (Join-Path $env:APPDATA 'rhizomind-audit-test')))

# --- Прогон на выпускной сборке ----------------------------------------------

$out = Join-Path $base 'audit-unattended'
if (Test-Path -LiteralPath $out) { Remove-Item -LiteralPath $out -Recurse -Force }
$started = Get-Date
$code = Invoke-Audit @('-Unattended', '-NoProcmon', '-AppExe', $AppExe, '-IdleSeconds', "$IdleSeconds", '-OutDir', $out)
Write-Host ("  прогон занял {0:N0} с" -f ((Get-Date) - $started).TotalSeconds)
Check 'прогон -Unattended -NoProcmon: код выхода 0' ($code -eq 0) "code=$code"

$results = Join-Path $out 'results.md'
Check 'results.md создан' (Test-Path -LiteralPath $results)
if (Test-Path -LiteralPath $results) {
  $text = Get-Content -Raw -Encoding UTF8 -LiteralPath $results
  Write-Host '----- results.md -----'
  Write-Host $text
  Write-Host '----------------------'
  $sections = @('Подготовка', 'Снимок до запуска', 'Запуск Process Monitor', 'Запуск Rhizomind и проверка флагов движка',
    'Простой: фоновые обращения движка', 'Сценарий: хранилище и заметки', 'Сценарий: поля ввода, контекстное меню, настройки',
    'Сценарий: внешние ссылки', 'Второе хранилище и выход', 'Остановка Process Monitor и разбор журнала', 'Итог')
  for ($i = 0; $i -lt $sections.Count; $i++) {
    $h = "## Шаг $($i + 1). $($sections[$i])"
    Check "results.md: раздел «$h»" ($text.Contains($h))
  }
  foreach ($flag in @('--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection', '--no-proxy-server', '--disable-background-networking',
      '--disable-component-update', '--disable-domain-reliability', '--no-pings', '--host-resolver-rules=', '--edge-webview-disable-crash-reporting=1')) {
    Check "results.md: флаг движка $flag — есть" ($text.Contains("| ``$flag`` | есть | есть |"))
  }
  Check 'results.md: --remote-debugging-port — нет' ($text.Contains('| `--remote-debugging-port` | нет | нет |'))
  Check 'results.md: нарушений флагов нет' (-not ($text -match '\*\*нарушение\*\*'))
  Check 'results.md: папка движка во временной папке' ($text.Contains('rhizomind-webview-*``: True') -or $text.Contains('rhizomind-webview-*`: True'))
  Check 'results.md: командная строка движка с %TEMP% вместо пути' ($text -match '--user-data-dir="?%TEMP%\\rhizomind-webview-')
  Check 'results.md: имени пользователя нет' ($text.IndexOf($env:USERNAME, [StringComparison]::OrdinalIgnoreCase) -lt 0)
  Check 'results.md: простой' ($text.Contains("- Простой: $IdleSeconds с."))
  Check 'results.md: процессы завершились после закрытия окна' ($text.Contains('- Процессы Rhizomind после выхода: 0'))
  Check 'results.md: временная папка движка удалена' ($text.Contains('- Новые временные папки движка после выхода: 0'))
  Check 'results.md: очередь WER описана' ($text -match '- Очередь WER после выхода: %LOCALAPPDATA%')
  Check 'results.md: итог по сети приложения' ($text.Contains('**Итог по сети приложения:**'))
  Check 'results.md: аудит дошёл до конца' ($text.Contains('Аудит завершён:'))
}
$left = @(Get-Process -Name 'rhizomind' -ErrorAction SilentlyContinue)
Check 'после прогона Rhizomind не запущен' ($left.Count -eq 0)
$left | Stop-Process -Force -ErrorAction SilentlyContinue

if ($script:Failures -gt 0) { Write-Host "ИТОГ: провалено $($script:Failures)"; exit 1 }
Write-Host 'ИТОГ: неинтерактивный прогон дошёл до results.md, разделы и флаги движка на месте'
exit 0

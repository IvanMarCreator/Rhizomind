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

# Настройки Process Monitor (реестр HKCU) скрипт на время аудита сбрасывает и
# возвращает (задача 3.3в). Здесь в ключе — метка «как будто фильтр владельца».
$pmKey = 'HKCU:\Software\Sysinternals\Process Monitor'
$pmKeyExisted = Test-Path -LiteralPath $pmKey
$pmBackup = Join-Path $base 'procmon-key-before.reg'
if ($pmKeyExisted) { Start-Process -FilePath 'reg.exe' -ArgumentList @('export', '"HKCU\Software\Sysinternals\Process Monitor"', "`"$pmBackup`"", '/y') -Wait -WindowStyle Hidden }
New-Item -Path $pmKey -Force | Out-Null
New-ItemProperty -LiteralPath $pmKey -Name 'AuditTestMarker' -PropertyType String -Value 'owner-filter' -Force | Out-Null

# Сбой после запуска Process Monitor: finally должен остановить его (/Terminate).
# Вместо Process Monitor — заглушка .cmd, которая записывает свои аргументы и
# метку из реестра в момент запуска записи; процесса Procmon после неё нет —
# скрипт останавливается на шаге 3 (пробная запись).
$stubDir = Join-Path $base 'procmon-stub'
New-Item -ItemType Directory -Force -Path $stubDir | Out-Null
$stub = Join-Path $stubDir 'Procmon64.cmd'
$calls = Join-Path $stubDir 'calls.log'
Remove-Item -LiteralPath $calls -Force -ErrorAction SilentlyContinue
Set-Content -LiteralPath $stub -Encoding ASCII -Value @(
  '@echo off',
  'echo %* >> "%~dp0calls.log"',
  'echo %* | findstr /C:"/BackingFile" >nul && reg query "HKCU\Software\Sysinternals\Process Monitor" /v AuditTestMarker >> "%~dp0calls.log" 2>&1',
  'exit /b 0')
$outStub = Join-Path $base 'audit-stub'
$code = Invoke-Audit @('-Unattended', '-AppExe', $AppExe, '-Procmon', $stub, '-ProcmonConfig', 'C:\old\rhizomind-audit.pmc', '-OutDir', $outStub)
$log = ''
if (Test-Path -LiteralPath $calls) { $log = Get-Content -Raw -LiteralPath $calls }
Write-Host "  вызовы заглушки Process Monitor: $($log -replace '\s+$', '')"
Check 'сбой после запуска Process Monitor — выход с кодом 2' ($code -eq 2) "code=$code"
Check 'пробная запись: Process Monitor запускался с /BackingFile procmon-probe.pml' ($log -match '/BackingFile\s+"?[^"]*procmon-probe\.pml')
Check 'Process Monitor запускался с /NoFilter' ($log -match '/NoFilter')
Check 'старый ключ -ProcmonConfig принят, но /LoadConfig не передаётся' ($log -notmatch '/LoadConfig')
Check 'во время записи настроек владельца в реестре нет (ключ сброшен)' ($log -notmatch 'owner-filter')
Check 'finally остановил Process Monitor (/Terminate)' ($log -match '/Terminate')
Check 'Rhizomind при сбое на шаге 3 не запускался' (@(Get-Process -Name 'rhizomind' -ErrorAction SilentlyContinue).Count -eq 0)
Check 'копия настроек Process Monitor — в папке результатов' (Test-Path -LiteralPath (Join-Path $outStub 'procmon-settings-backup.reg'))
Check 'после сбоя настройки Process Monitor возвращены (метка на месте)' ((Get-ItemProperty -LiteralPath $pmKey -ErrorAction SilentlyContinue).AuditTestMarker -eq 'owner-filter')
Check 'после сбоя в ключе нет значений, оставленных скриптом' ($null -eq (Get-ItemProperty -LiteralPath $pmKey -ErrorAction SilentlyContinue).ResolveAddresses)

# Пробная запись не видит тестового процесса (как журнал первого аудита):
# аудит останавливается до запуска Rhizomind с понятной причиной. Заглушка
# запускает копию ping.exe под именем Procmon64.exe (процесс Process Monitor
# виден), на выгрузку пробной записи кладёт CSV без событий тестового процесса.
function New-ProcmonFake([string] $dir, [string] $probeFixture, [string] $outDir) {
  New-Item -ItemType Directory -Force -Path $dir | Out-Null
  Copy-Item -LiteralPath (Join-Path $env:SystemRoot 'System32\PING.EXE') -Destination (Join-Path $dir 'Procmon64.exe') -Force
  Copy-Item -LiteralPath (Join-Path $PSScriptRoot "fixtures\$probeFixture") -Destination (Join-Path $dir 'probe.csv') -Force
  Remove-Item -LiteralPath (Join-Path $dir 'calls.log') -Force -ErrorAction SilentlyContinue
  $fake = Join-Path $dir 'Procmon-stub.cmd'
  Set-Content -LiteralPath $fake -Encoding ASCII -Value @(
    '@echo off',
    'echo %* >> "%~dp0calls.log"',
    'echo %* | findstr /C:"/BackingFile" >nul && start "" "%~dp0Procmon64.exe" -n 900 127.0.0.1',
    'echo %* | findstr /C:"/Terminate" >nul && taskkill /F /IM Procmon64.exe >nul 2>&1',
    ('echo %* | findstr /C:"procmon-probe.csv" >nul && copy /y "%~dp0probe.csv" "' + $outDir + '\procmon-probe.csv" >nul'),
    'exit /b 0')
  if (Test-Path -LiteralPath $outDir) { Remove-Item -LiteralPath $outDir -Recurse -Force }
  return $fake
}

$outBad = Join-Path $base 'audit-badprobe'
$fakeBad = New-ProcmonFake (Join-Path $base 'procmon-badprobe') 'procmon-no-app.csv' $outBad
$code = Invoke-Audit @('-Unattended', '-AppExe', $AppExe, '-Procmon', $fakeBad, '-OutDir', $outBad)
Check 'проба без событий тестового процесса — выход с кодом 2' ($code -eq 2) "code=$code"
$badText = ''
if (Test-Path -LiteralPath (Join-Path $outBad 'results.md')) { $badText = Get-Content -Raw -Encoding UTF8 -LiteralPath (Join-Path $outBad 'results.md') }
Check 'проба без событий: в results.md причина' ($badText.Contains('**Пробная запись не прошла:** в пробном журнале нет ни одного события тестового процесса cmd.exe'))
Check 'проба без событий: в results.md классы событий пробы' ($badText -match 'Пробная запись: событий всего 5 \(файлы 0, реестр 0, процессы 0, сеть 5')
Check 'проба без событий: аудит прерван до Rhizomind' ($badText.Contains('**Аудит прерван:** Пробная запись Process Monitor не прошла') -and -not $badText.Contains('## Шаг 4.'))
Check 'проба без событий: Rhizomind не запускался' (@(Get-Process -Name 'rhizomind' -ErrorAction SilentlyContinue).Count -eq 0)
Check 'проба без событий: Process Monitor остановлен' (@(Get-Process -Name 'Procmon64' -ErrorAction SilentlyContinue).Count -eq 0)
Check 'проба без событий: настройки Process Monitor возвращены' ((Get-ItemProperty -LiteralPath $pmKey -ErrorAction SilentlyContinue).AuditTestMarker -eq 'owner-filter')
Get-Process -Name 'Procmon64', 'rhizomind' -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue

# Пробная запись прошла, Process Monitor «записывал», но CSV основного журнала
# не получился: итог не должен говорить «обращений не найдено».
$outNoCsv = Join-Path $base 'audit-nocsv'
$fakeDir = Join-Path $base 'procmon-nocsv'
$fakeStub = New-ProcmonFake $fakeDir 'procmon-probe-ok.csv' $outNoCsv
$code = Invoke-Audit @('-Unattended', '-AppExe', $AppExe, '-Procmon', $fakeStub, '-IdleSeconds', '5', '-OutDir', $outNoCsv)
Check 'Process Monitor без CSV: скрипт доходит до конца (код 0)' ($code -eq 0) "code=$code"
$noCsvText = ''
if (Test-Path -LiteralPath (Join-Path $outNoCsv 'results.md')) { $noCsvText = Get-Content -Raw -Encoding UTF8 -LiteralPath (Join-Path $outNoCsv 'results.md') }
$fakeLog = Get-Content -Raw -LiteralPath (Join-Path $fakeDir 'calls.log')
Check 'проба прошла: в results.md классы событий тестового процесса' ($noCsvText -match 'тестового процесса cmd\.exe \(PID [0-9, ]+\): файлы 2, реестр 2, процессы 4')
Check 'основная запись: /BackingFile capture.pml с /NoFilter' ($fakeLog -match '/NoFilter /BackingFile\s+"?[^"]*capture\.pml')
Check 'выгрузка: /OpenLog … /SaveAs с /NoFilter, без /SaveApplyFilter' ($fakeLog -match '/NoFilter /OpenLog\s+"?[^"]*capture\.pml"?\s+/SaveAs' -and $fakeLog -notmatch '/SaveApplyFilter')
Check 'проверка через 60 с: Process Monitor работает' ($noCsvText.Contains('- Через 60 с после запуска Rhizomind: Process Monitor работает — True'))
Check 'Process Monitor без CSV: итог «не доказано» с причиной' ($noCsvText.Contains('**Итог по сети приложения:** **не доказано** — журнал не выгружен в CSV (файла capture.csv нет).'))
Check 'Process Monitor без CSV: нет ложного «обращений не найдено»' (-not $noCsvText.Contains('обращений процессов Rhizomind и движка не найдено'))
Check 'Process Monitor без CSV: заглушка остановлена (/Terminate)' ($fakeLog -match '/Terminate')
Check 'после аудита настройки Process Monitor возвращены' ((Get-ItemProperty -LiteralPath $pmKey -ErrorAction SilentlyContinue).AuditTestMarker -eq 'owner-filter')
Get-Process -Name 'Procmon64', 'rhizomind' -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue

# Ключ Process Monitor раннера — как был до тестов.
Remove-Item -LiteralPath $pmKey -Recurse -Force -ErrorAction SilentlyContinue
if ($pmKeyExisted) { Start-Process -FilePath 'reg.exe' -ArgumentList @('import', "`"$pmBackup`"") -Wait -WindowStyle Hidden }

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

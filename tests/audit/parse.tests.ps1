# Модульные тесты помощника сетевого аудита (задача 3.3б, решение владельца 102):
# разбор журнала Process Monitor на образце CSV и мелкие функции.
# Запуск: powershell -NoProfile -ExecutionPolicy Bypass -File tests/audit/parse.tests.ps1
# Без Pester: каждая проверка печатает OK/FAIL, при любой ошибке код выхода 1.
$ErrorActionPreference = 'Stop'
try { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8 } catch { $null = $_ }

$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$auditScript = Join-Path $root 'docs\security\network-audit.ps1'
. $auditScript -LibraryOnly

$script:Failures = 0
function Check([string] $name, [bool] $condition, [string] $detail = '') {
  if ($condition) { Write-Host "OK   $name" } else { Write-Host "FAIL $name $detail"; $script:Failures++ }
}

Write-Host "PowerShell $($PSVersionTable.PSVersion) ($($PSVersionTable.PSEdition))"

# --- Разбор образца ----------------------------------------------------------

$user = 'C:\Users\Тестовый'
$engineShort = 'C:\Users\5D3E~1\AppData\Local\Temp\rhizomind-webview-4242-0123456789abcdef'
$r = Invoke-ProcmonAnalysis -CsvPath (Join-Path $PSScriptRoot 'fixtures\procmon-sample.csv') `
  -OwnPids @(1000) -VaultRoots @("$user\Документы\Хранилище") -EngineDir $engineShort `
  -AppDataDir "$user\AppData\Local\Rhizomind"

function Find-Key([hashtable] $table, [string] $pattern) { return @($table.Keys | Where-Object { $_ -like $pattern }) }

Check 'разбор: прочитаны все 23 события' ($r.Rows -eq 23) "rows=$($r.Rows)"
Check 'разбор: дерево процессов по Process Create (1000 -> 2000 -> 2001)' ($r.Own.Contains(2000) -and $r.Own.Contains(2001))
Check 'разбор: WebView2 другого приложения (5000) не считается своим' (-not $r.Own.Contains(5000))

# Сеть
Check 'сеть: TCP процесса движка — в таблице сети приложения' ((Find-Key $r.NetOwn "msedgewebview2.exe`tTCP`t13.107.42.14:443").Count -eq 1) ($r.NetOwn.Keys -join ' | ')
Check 'сеть: UDP на порт 53 процесса движка — в таблице сети приложения' ((Find-Key $r.NetOwn "msedgewebview2.exe`tUDP`t10.0.0.1:53").Count -eq 1)
Check 'сеть: в таблице сети приложения ровно 2 строки (чужой WebView2 и loopback не считаются)' ($r.NetOwn.Count -eq 2) "count=$($r.NetOwn.Count)"
Check 'сеть: запросов DNS от движка — 1 (--host-resolver-rules был бы опровергнут)' ($r.DnsOwn -eq 1 -and $r.DnsAll -eq 1) "own=$($r.DnsOwn) all=$($r.DnsAll)"
Check 'сеть: своё обращение — оценка «нарушение»' ((Find-Key $r.NetAll "msedgewebview2.exe`tTCP`t13.107.42.14:443`tнарушение*").Count -eq 1)
Check 'сеть: svchost к 20.190.x.x — служба входа Microsoft (решение 64)' ((Find-Key $r.NetAll "svchost.exe`tTCP`t20.190.151.68:443`tслужба входа Microsoft*").Count -eq 1)
Check 'сеть: svchost к прочему адресу — служба Windows, сверить адрес (разрешённая категория)' ((Find-Key $r.NetAll "svchost.exe`tTCP`t52.168.112.66:443`tслужба Windows из SECURITY.md*").Count -eq 1)
Check 'сеть: неизвестный процесс — «разобрать» (не из списка)' ((Find-Key $r.NetAll "someapp.exe`tTCP`t93.184.216.34:443`tне из списка SECURITY.md*").Count -eq 1)
Check 'сеть: чужой WebView2 — контроль для флагов, не нарушение' ((Find-Key $r.NetAll "msedgewebview2.exe`tTCP`t13.107.42.14:443`tWebView2 другого приложения*").Count -eq 1)
Check 'сеть: loopback не учитывается' ((Find-Key $r.NetAll '*127.0.0.1*').Count -eq 0)

# Файлы
$files = @($r.FileOut.Keys)
Check 'файлы: временная папка движка в полной форме при командной строке в форме 8.3 — не нарушение' ((@($files | Where-Object { $_ -like '*rhizomind-webview-4242*' })).Count -eq 0) ($files -join ' | ')
Check 'файлы: хранилище и %LOCALAPPDATA%\Rhizomind — не нарушение' ((@($files | Where-Object { $_ -like '*Хранилище*' -or $_ -like '*vaults.json*' })).Count -eq 0)
Check 'файлы: запись вне хранилища (Roaming) — «не учтено»' ((Find-Key $r.FileOut "$user\AppData\Roaming\Rhizomind\leak.txt`t**не учтено**").Count -eq 1)
Check 'файлы: корзина ОС — учтено' ((Find-Key $r.FileOut "C:\`$Recycle.Bin\*`tкорзина ОС*").Count -eq 1)
Check 'файлы: TokenBroker от движка — помечен «должно отсутствовать»' ((Find-Key $r.FileOut "*TokenBroker\Cache\e8dd*`t*должно отсутствовать*").Count -eq 1)
Check 'файлы: отказ в доступе и открытие на чтение не считаются' ((@($files | Where-Object { $_ -like 'C:\Windows\*' })).Count -eq 0)
Check 'файлы: чужой процесс не считается' ((@($files | Where-Object { $_ -like '*\Other\*' })).Count -eq 0)
Check 'файлы: в таблице ровно 3 строки' ($r.FileOut.Count -eq 3) "count=$($r.FileOut.Count)"
Check 'файлы: словарь Hunspell во временной папке движка — отдельная находка' ($r.Dictionaries.Count -eq 1 -and (@($r.Dictionaries.Keys)[0] -like '*ru-RU-3-0.bdic'))

# Реестр
Check 'реестр: EdgeWebView — учтено (решение 61)' ((Find-Key $r.RegOut "HKCU\Software\Microsoft\EdgeWebView\StabilityMetrics`tдвижок WebView2, решение 61").Count -eq 1) ($r.RegOut.Keys -join ' | ')
Check 'реестр: неизвестный ключ — «не учтено»' ((Find-Key $r.RegOut "HKCU\Software\Rhizomind\Unknown`t**не учтено**").Count -eq 1)

# Журнал без событий Rhizomind (старый фильтр Process Monitor) — разбор должен
# это видеть, иначе итог «обращений не найдено» был бы ложным.
Check 'разбор: событий корневого процесса Rhizomind в образце — есть' ($r.RootRows -gt 0) "root=$($r.RootRows)"
$rNoRoot = Invoke-ProcmonAnalysis -CsvPath (Join-Path $PSScriptRoot 'fixtures\procmon-sample.csv') -OwnPids @(9999) -AppDataDir 'C:\nonexistent'
Check 'разбор: журнал без строк с PID корневого процесса — RootRows = 0' ($rNoRoot.RootRows -eq 0) "root=$($rNoRoot.RootRows)"
Check 'итог: журнал без событий Rhizomind — «не доказано»' ((Get-ProcmonVerdict -NoProcmon:$false -Analysis $rNoRoot) -eq 'not-proven')
Check 'итог: журнал не разобран — «не доказано»' ((Get-ProcmonVerdict -NoProcmon:$false -Analysis $null) -eq 'not-proven')
Check 'итог: журнал с событиями Rhizomind — «разобран»' ((Get-ProcmonVerdict -NoProcmon:$false -Analysis $r) -eq 'analysed')
Check 'итог: -NoProcmon — «без журнала»' ((Get-ProcmonVerdict -NoProcmon:$true -Analysis $null) -eq 'no-procmon')

# --- Полнота журнала (задача 3.3в, решение 107а) -----------------------------

Check 'классы: Get-OperationClass' (
  (Get-OperationClass 'RegQueryValue') -eq 'Registry' -and (Get-OperationClass 'TCP Send') -eq 'Network' -and
  (Get-OperationClass 'UDP Receive') -eq 'Network' -and (Get-OperationClass 'Process Start') -eq 'Process' -and
  (Get-OperationClass 'Thread Exit') -eq 'Process' -and (Get-OperationClass 'Load Image') -eq 'Process' -and
  (Get-OperationClass 'Process Profiling') -eq 'Profiling' -and (Get-OperationClass 'CreateFile') -eq 'File' -and
  (Get-OperationClass 'IRP_MJ_CLOSE') -eq 'File')
Check 'классы: в образце есть все четыре класса' ($r.Classes.File -gt 0 -and $r.Classes.Registry -gt 0 -and $r.Classes.Process -gt 0 -and $r.Classes.Network -gt 0) (Format-ClassCounter $r.Classes)
Check 'классы: сумма по классам = числу событий' (($r.Classes.File + $r.Classes.Registry + $r.Classes.Process + $r.Classes.Network + $r.Classes.Profiling) -eq $r.Rows)
Check 'классы: у корневого процесса посчитаны' ($r.RootClasses.Process -gt 0) (Format-ClassCounter $r.RootClasses)
Check 'полнота: образец — причин «не доказано» нет' (@(Get-ProcmonGap $r).Count -eq 0) (@(Get-ProcmonGap $r) -join '; ')
Check 'полнота: столбцы образца на месте' (@($r.MissingColumns).Count -eq 0)
Check 'контроль: события на пути TokenBroker\Cache посчитаны' ($r.TokenBrokerRows -ge 1) "tb=$($r.TokenBrokerRows)"

# Журнал как в первом аудите владельца: сеть других процессов есть, событий
# Rhizomind, реестра и процессов нет.
$rOwner = Invoke-ProcmonAnalysis -CsvPath (Join-Path $PSScriptRoot 'fixtures\procmon-no-app.csv') -OwnPids @(19544) -AppDataDir 'C:\nonexistent'
$gaps = @(Get-ProcmonGap $rOwner)
Check 'журнал владельца: итог «не доказано»' ((Get-ProcmonVerdict -NoProcmon:$false -Analysis $rOwner) -eq 'not-proven')
Check 'журнал владельца: причина — нет событий корневого процесса с номером' (@($gaps | Where-Object { $_ -like '*корневого процесса Rhizomind (PID 19544)*' }).Count -eq 1) ($gaps -join '; ')
Check 'журнал владельца: причина — нет событий реестра' (@($gaps | Where-Object { $_ -like '*событий реестра*' }).Count -eq 1)
Check 'журнал владельца: причина — нет событий процессов' (@($gaps | Where-Object { $_ -like '*событий процессов*' }).Count -eq 1)
Check 'журнал владельца: сеть и файлы — не в причинах (сеть есть)' (@($gaps | Where-Object { $_ -like '*сетевых*' }).Count -eq 0)
Check 'журнал не выгружен: конкретная причина' ((@(Get-ProcmonGap $null) -join '') -like '*capture.csv нет*')

$tmp = Join-Path $env:TEMP ('audit-tests-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
  $badCols = Join-Path $tmp 'cols.csv'
  @('"Time of Day","Process Name","Operation"', '"10:00:00","rhizomind.exe","CreateFile"') | Set-Content -LiteralPath $badCols -Encoding UTF8
  $rCols = Invoke-ProcmonAnalysis -CsvPath $badCols -OwnPids @(1) -AppDataDir 'C:\nonexistent'
  Check 'полнота: CSV без столбцов PID, Path… — причина названа' ((@(Get-ProcmonGap $rCols) -join '; ') -like '*нет столбцов PID, Path, Result, Detail*') (@(Get-ProcmonGap $rCols) -join '; ')

  # Пробная запись
  $pOk = Test-ProcmonProbe -CsvPath (Join-Path $PSScriptRoot 'fixtures\procmon-probe-ok.csv') -ProbePid 99999
  Check 'проба: тестовый процесс найден по метке, классы File, Registry, Process есть — проходит' ($pOk.Ok -and ($pOk.Pids -contains 4242)) "$($pOk.Reason) pids=$($pOk.Pids -join ',')"
  Check 'проба: события тестового процесса посчитаны по классам' ($pOk.Classes.File -eq 2 -and $pOk.Classes.Registry -eq 2 -and $pOk.Classes.Process -eq 4) (Format-ClassCounter $pOk.Classes)
  $pOwner = Test-ProcmonProbe -CsvPath (Join-Path $PSScriptRoot 'fixtures\procmon-no-app.csv') -ProbePid 4242
  Check 'проба: журнал без событий тестового процесса — не проходит, причина «отбрасывает события»' ((-not $pOwner.Ok) -and $pOwner.Reason -like '*нет ни одного события тестового процесса*отбрасывает*') $pOwner.Reason
  $fileOnly = Join-Path $tmp 'probe-file-only.csv'
  @('"Time of Day","Process Name","PID","Operation","Path","Result","Detail"',
    '"10:00:00","cmd.exe","77","CreateFile","C:\x\procmon-probe.txt","SUCCESS","OpenResult: Created"') | Set-Content -LiteralPath $fileOnly -Encoding UTF8
  $pFile = Test-ProcmonProbe -CsvPath $fileOnly -ProbePid 77
  Check 'проба: только файловые события — не проходит, названы классы Registry, Process' ((-not $pFile.Ok) -and $pFile.Reason -like '*классов Registry, Process*') $pFile.Reason
  $pNone = Test-ProcmonProbe -CsvPath (Join-Path $tmp 'nope.csv') -ProbePid 77
  Check 'проба: CSV нет — не проходит' ((-not $pNone.Ok) -and $pNone.Reason -like '*не выгрузилась*') $pNone.Reason

  # Номер rhizomind.exe из results.md (имя канала движка)
  Check 'RootPid из results.md: --mojo-named-platform-channel-pipe=19544.…' ((Get-RootPidFromResult 'x --mojo-named-platform-channel-pipe=19544.7780.123456789 --y') -eq 19544)
  Check 'RootPid из results.md: канала нет — 0' ((Get-RootPidFromResult 'nothing') -eq 0)

  # Режим только разбора (-AnalyzeOnly) отдельным процессом PowerShell
  $exe = (Get-Process -Id $PID).Path
  $out1 = Join-Path $tmp 'reanalysis-sample'
  & $exe -NoProfile -ExecutionPolicy Bypass -File $auditScript -AnalyzeOnly -Csv (Join-Path $PSScriptRoot 'fixtures\procmon-sample.csv') -RootPid 1000 -OutDir $out1 | ForEach-Object { Write-Host "  | $_" }
  Check '-AnalyzeOnly на образце: код 0' ($LASTEXITCODE -eq 0) "code=$LASTEXITCODE"
  $t1 = ''
  if (Test-Path -LiteralPath (Join-Path $out1 'results.md')) { $t1 = Get-Content -Raw -Encoding UTF8 -LiteralPath (Join-Path $out1 'results.md') }
  Check '-AnalyzeOnly: results.md с классами событий' ($t1 -like '*Классы событий в журнале (все процессы): файлы *, реестр *, процессы *, сеть *') $t1
  Check '-AnalyzeOnly: в образце сеть движка — итог «нарушение»' ($t1 -like '*найдено * обращений — нарушение*')
  Check '-AnalyzeOnly: временная папка движка найдена по командной строке в журнале' ($t1 -like '*rhizomind-webview-4242-0123456789abcdef*')
  Check '-AnalyzeOnly: записи движка во временной папке не попали в «не учтено»' (-not ((Get-Content -Raw -Encoding UTF8 -LiteralPath (Join-Path $out1 'summary-files.csv')) -like '*rhizomind-webview-4242*'))

  # Журнал как у владельца; RootPid — из results.md рядом с CSV
  $ownerDir = Join-Path $tmp 'rhizomind-audit-2026-11-01-1922'
  New-Item -ItemType Directory -Path $ownerDir | Out-Null
  Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'fixtures\procmon-no-app.csv') -Destination (Join-Path $ownerDir 'capture.csv')
  Set-Content -LiteralPath (Join-Path $ownerDir 'results.md') -Encoding UTF8 -Value '```', 'msedgewebview2.exe --embedded-browser-webview=1 --mojo-named-platform-channel-pipe=19544.20112.5822364170375591133', '```'
  & $exe -NoProfile -ExecutionPolicy Bypass -File $auditScript -AnalyzeOnly -Csv (Join-Path $ownerDir 'capture.csv') | ForEach-Object { Write-Host "  | $_" }
  Check '-AnalyzeOnly без -RootPid: код 0' ($LASTEXITCODE -eq 0) "code=$LASTEXITCODE"
  $re = @(Get-ChildItem -LiteralPath $ownerDir -Directory -Filter 'reanalysis-*')
  Check '-AnalyzeOnly: итог в подпапке reanalysis-<дата>, исходный results.md не тронут' ($re.Count -eq 1 -and (Get-Content -Raw -LiteralPath (Join-Path $ownerDir 'results.md')) -like '*19544.20112*')
  $t2 = ''
  if ($re.Count -eq 1) { $t2 = Get-Content -Raw -Encoding UTF8 -LiteralPath (Join-Path $re[0].FullName 'results.md') }
  Check '-AnalyzeOnly: RootPid 19544 взят из results.md' ($t2 -like '*PID 19544 (источник: results.md*') $t2
  Check '-AnalyzeOnly: итог «не доказано» с конкретной причиной' ($t2 -like '***не доказано** — нет ни одного события корневого процесса Rhizomind (PID 19544)*')

  & $exe -NoProfile -ExecutionPolicy Bypass -File $auditScript -AnalyzeOnly -Csv (Join-Path $tmp 'nope.csv') -RootPid 1 | ForEach-Object { Write-Host "  | $_" }
  Check '-AnalyzeOnly: CSV нет — код 2' ($LASTEXITCODE -eq 2) "code=$LASTEXITCODE"
} finally {
  Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
}

# --- Мелкие функции -----------------------------------------------------------

Check 'д/н: «д», «Да», «l», «yes» — да' (@('д', 'Да', 'l', 'YES' | ForEach-Object { ConvertTo-YesNo $_ }) -notcontains $false)
Check 'д/н: одиночное «y» («н» в английской раскладке) — не ответ' ($null -eq (ConvertTo-YesNo 'y'))
Check 'д/н: «н», «нет», «n», «т» — нет' (@('н', 'нет', 'n', 'т' | ForEach-Object { ConvertTo-YesNo $_ }) -notcontains $true)
Check 'д/н: «может» — не ответ' ($null -eq (ConvertTo-YesNo 'может'))
Check 'путь: кавычки «Копировать как путь» убираются' ((ConvertFrom-TypedPath ' "C:\Tools\Procmon64.exe" ') -eq 'C:\Tools\Procmon64.exe')

$tb = '01c5cb21ab1d4fd56e65159d6c36cd5db1f647e1.tbres'
$changes = Compare-TokenCache @{ $tb = '1'; 'a.tbres' = '1' } @{ $tb = '2'; 'a.tbres' = '1'; 'b.tbres' = '1' }
Check 'TokenBroker: изменённый 01c5cb21….tbres — фон Windows, учтено' (@($changes | Where-Object { $_.Name -eq $tb -and -not $_.Violation -and $_.Verdict -like 'фон Windows*' }).Count -eq 1)
Check 'TokenBroker: новый другой файл — нарушение' (@($changes | Where-Object { $_.Name -eq 'b.tbres' -and $_.Violation }).Count -eq 1)
$removed = Compare-TokenCache @{ $tb = '1' } @{}
Check 'TokenBroker: удаление 01c5cb21….tbres — нарушение (его удаляет OneAuth)' (@($removed | Where-Object { $_.Change -eq 'удалён' -and $_.Violation }).Count -eq 1)

function New-FakeDnsEvent([string] $name) {
  $o = [pscustomobject]@{ Name = $name }
  $o | Add-Member -MemberType ScriptMethod -Name ToXml -Value {
    "<Event xmlns='http://schemas.microsoft.com/win/2004/08/events/event'><EventData><Data Name='QueryName'>$($this.Name)</Data><Data Name='QueryType'>1</Data></EventData></Event>"
  }
  return $o
}
$dnsNames = @(Get-DnsQueryName @((New-FakeDnsEvent 'wpad'), (New-FakeDnsEvent 'config.edge.skype.com'), (New-FakeDnsEvent 'wpad')))
Check 'DNS: имя без точки (wpad) берётся из XML события' ($dnsNames -contains 'wpad') ($dnsNames -join ', ')
Check 'DNS: имена без повторов' ($dnsNames.Count -eq 2)

# Короткие пути 8.3 на настоящем диске: в CI %TEMP% = C:\Users\RUNNER~1\...
$forms = @(Get-PathForms $env:TEMP)
Write-Host "  %TEMP% в формах: $($forms -join ' | ')"
if ($env:TEMP -like '*~*') {
  Check 'пути 8.3: у %TEMP% в короткой форме найдена полная форма' (@($forms | Where-Object { $_ -notlike '*~*' }).Count -ge 1)
} else {
  Write-Host 'SKIP пути 8.3: %TEMP% уже в полной форме на этой машине'
}
$probe = Join-Path $env:TEMP ('rhizomind-webview-1-' + ('{0:x16}' -f (Get-Random -Maximum ([long]::MaxValue))))
New-Item -ItemType Directory -Path $probe | Out-Null
try {
  $long = Get-LongPath $probe
  Check 'пути 8.3: полная форма существующей папки сохраняет её имя' ($long -and (Split-Path -Leaf $long) -eq (Split-Path -Leaf $probe)) "long=$long"
  $csv = Join-Path $probe 'sample.csv'
  @(
    '"Time of Day","Process Name","PID","Operation","Path","Result","Detail"',
    ('"10:00:00","msedgewebview2.exe","7","WriteFile","{0}\EBWebView\Local State","SUCCESS",""' -f $long)
  ) | Set-Content -LiteralPath $csv -Encoding UTF8
  $r2 = Invoke-ProcmonAnalysis -CsvPath $csv -OwnPids @(7) -EngineDir $probe -AppDataDir 'C:\nonexistent'
  Check 'пути 8.3: запись в папку движка (журнал — полная форма, командная строка — как в %TEMP%) не нарушение' ($r2.FileOut.Count -eq 0) ($r2.FileOut.Keys -join ' | ')
} finally {
  Remove-Item -LiteralPath $probe -Recurse -Force
}

# Папка результатов не под OneDrive
$savedOneDrive = $env:OneDrive
try {
  $env:OneDrive = Join-Path $env:TEMP 'FakeOneDrive'
  Check 'OneDrive: папка под OneDrive — синхронизируется' (Test-SyncedPath (Join-Path $env:OneDrive 'rhizomind-audit'))
  Check 'OneDrive: C:\rhizomind-audit — нет' (-not (Test-SyncedPath 'C:\rhizomind-audit-2026-11-01'))
  Check 'OneDrive: перемещаемый профиль %APPDATA% — синхронизируется' (Test-SyncedPath (Join-Path $env:APPDATA 'x'))
} finally {
  $env:OneDrive = $savedOneDrive
}

$masked = Protect-UserText ("--user-data-dir=`"" + $env:TEMP + "\rhizomind-webview-1-0\EBWebView`" " + $env:USERPROFILE + '\x')
Check 'results.md: путь %TEMP% заменён условным обозначением' ($masked -like '*%TEMP%\rhizomind-webview-1-0*') $masked
Check 'results.md: имени пользователя нет' ($masked -notlike "*$env:USERNAME*") $masked

if ($script:Failures -gt 0) { Write-Host "ИТОГ: провалено $($script:Failures)"; exit 1 }
Write-Host 'ИТОГ: разбор образца и функции — все проверки пройдены'
exit 0

# Помощник ручного сетевого аудита Rhizomind (план 0.1, задача 3.3).
# Инструкция — docs/security/network-audit.md. Скрипт ведёт по шагам, сам
# снимает то, что можно снять командами, и собирает итог в results.md.
#
# Запуск (Windows PowerShell 5.1 или PowerShell 7, обычный пользователь;
# от администратора — дополнительно журнал DNS):
#   powershell -ExecutionPolicy Bypass -File network-audit.ps1 `
#     -AppExe "C:\...\rhizomind.exe" -Procmon "C:\Tools\Procmon64.exe"
#
# Скрипт ничего не скачивает и в сеть не ходит. Он только читает: процессы,
# сетевые соединения, реестр, журнал DNS, файлы в профиле; пишет — в свою
# папку результатов. Удаляет он только то, что создал сам (CSV-выгрузку можно
# оставить). Журнал Process Monitor (.pml) содержит всё, что делала машина:
# его не пересылайте, пересылайте results.md и summary-*.csv.
param(
  [string] $AppExe = '',
  [string] $Procmon = '',
  [string] $OutDir = ''
)

$ErrorActionPreference = 'Stop'
$TotalStages = 11
$script:Stage = 0

if (-not $OutDir) {
  $OutDir = Join-Path ([Environment]::GetFolderPath('Desktop')) ('rhizomind-audit-' + (Get-Date -Format 'yyyy-MM-dd-HHmm'))
}
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$Results = Join-Path $OutDir 'results.md'
$Pml = Join-Path $OutDir 'capture.pml'
$Csv = Join-Path $OutDir 'capture.csv'

# --- Вывод и вопросы ---------------------------------------------------------

function Stage([string] $title) {
  $script:Stage++
  Clear-Host
  Write-Host ('=' * 72) -ForegroundColor DarkCyan
  Write-Host ("Шаг {0} из {1}: {2}" -f $script:Stage, $TotalStages, $title) -ForegroundColor Cyan
  Write-Host ('=' * 72) -ForegroundColor DarkCyan
  Write-Host ''
  Add-Content -LiteralPath $Results -Encoding UTF8 -Value ("`n## Шаг {0}. {1}`n" -f $script:Stage, $title)
}

function Say([string] $text) { Write-Host $text }
function Step([string] $text) { Write-Host ('  > ' + $text) -ForegroundColor Yellow }
function Good([string] $text) { Write-Host ('  OK  ' + $text) -ForegroundColor Green }
function Bad([string] $text) { Write-Host ('  !!  ' + $text) -ForegroundColor Red }

function Note([string] $text) { Add-Content -LiteralPath $Results -Encoding UTF8 -Value $text }

function Wait-Enter([string] $text = 'Нажмите Enter, когда будет готово') {
  [void](Read-Host ("`n" + $text))
}

function Ask([string] $question, [string] $default = '') {
  $suffix = ''
  if ($default) { $suffix = " [$default]" }
  $answer = Read-Host ($question + $suffix)
  if (-not $answer) { $answer = $default }
  return $answer
}

function Confirm([string] $question) {
  while ($true) {
    $a = (Read-Host ($question + ' (д/н)')).Trim().ToLower()
    if ($a -in @('д', 'да', 'y', 'yes')) { return $true }
    if ($a -in @('н', 'нет', 'n', 'no')) { return $false }
  }
}

# --- Процессы Rhizomind ------------------------------------------------------

# Все когда-либо замеченные процессы приложения: номер -> имя и командная строка.
$script:Seen = @{}
$script:RootPid = 0

function Get-AppTree {
  if ($script:RootPid -eq 0) { return @() }
  $all = @(Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, Name, CommandLine)
  $ids = New-Object 'System.Collections.Generic.HashSet[int]'
  [void]$ids.Add([int]$script:RootPid)
  $changed = $true
  while ($changed) {
    $changed = $false
    foreach ($p in $all) {
      if ($ids.Contains([int]$p.ParentProcessId) -and -not $ids.Contains([int]$p.ProcessId)) {
        [void]$ids.Add([int]$p.ProcessId)
        $changed = $true
      }
    }
  }
  $tree = @($all | Where-Object { $ids.Contains([int]$_.ProcessId) })
  foreach ($p in $tree) {
    if (-not $script:Seen.ContainsKey([int]$p.ProcessId)) {
      $script:Seen[[int]$p.ProcessId] = $p
    }
  }
  return $tree
}

function Test-OwnImage([string] $name) {
  return ($name -ieq 'rhizomind.exe') -or ($name -ieq 'msedgewebview2.exe')
}

# Опрос сетевых соединений процессов приложения, пока не нажата клавиша
# (или не прошло $Minutes минут). Возвращает найденные соединения.
$script:LiveFindings = New-Object System.Collections.ArrayList

function Watch-Network([string] $phase, [int] $Minutes = 0) {
  $deadline = $null
  if ($Minutes -gt 0) { $deadline = (Get-Date).AddMinutes($Minutes) }
  $keys = @{}
  Say ''
  if ($deadline) {
    Say ("Слежу за сетью процессов Rhizomind {0} мин. Любая клавиша — закончить раньше." -f $Minutes)
  } else {
    Say 'Слежу за сетью процессов Rhizomind. Выполняйте действия; когда закончите — нажмите любую клавишу.'
  }
  while ($true) {
    $tree = Get-AppTree
    $own = @($tree | Where-Object { Test-OwnImage $_.Name } | ForEach-Object { [int]$_.ProcessId })
    $other = @($tree | Where-Object { -not (Test-OwnImage $_.Name) } | ForEach-Object { [int]$_.ProcessId })
    $tcp = @(Get-NetTCPConnection -ErrorAction SilentlyContinue | Where-Object {
        ($own -contains [int]$_.OwningProcess) -and $_.RemoteAddress -notin @('0.0.0.0', '::', '127.0.0.1', '::1')
      })
    $udp = @(Get-NetUDPEndpoint -ErrorAction SilentlyContinue | Where-Object { $own -contains [int]$_.OwningProcess })
    foreach ($c in $tcp) {
      $k = "TCP $($c.OwningProcess) $($c.RemoteAddress):$($c.RemotePort)"
      if (-not $keys.ContainsKey($k)) {
        $keys[$k] = $true
        $name = $script:Seen[[int]$c.OwningProcess].Name
        [void]$script:LiveFindings.Add([pscustomobject]@{ Phase = $phase; Kind = 'TCP'; Pid = $c.OwningProcess; Process = $name; Remote = "$($c.RemoteAddress):$($c.RemotePort)"; State = "$($c.State)" })
        Bad ("TCP: $name ($($c.OwningProcess)) -> $($c.RemoteAddress):$($c.RemotePort) [$($c.State)]")
      }
    }
    foreach ($u in $udp) {
      $k = "UDP $($u.OwningProcess) $($u.LocalAddress):$($u.LocalPort)"
      if (-not $keys.ContainsKey($k)) {
        $keys[$k] = $true
        $name = $script:Seen[[int]$u.OwningProcess].Name
        [void]$script:LiveFindings.Add([pscustomobject]@{ Phase = $phase; Kind = 'UDP (открыт порт)'; Pid = $u.OwningProcess; Process = $name; Remote = "$($u.LocalAddress):$($u.LocalPort)"; State = '' })
        Bad ("UDP-порт открыт: $name ($($u.OwningProcess)) $($u.LocalAddress):$($u.LocalPort)")
      }
    }
    if ($other.Count -gt 0) {
      foreach ($p in $other) {
        $k = "OTHER $p"
        if (-not $keys.ContainsKey($k)) {
          $keys[$k] = $true
          Say ("  (запущен процесс не из Rhizomind: {0}, {1} — например, браузер по ссылке; его сеть не считается)" -f $script:Seen[$p].Name, $p)
        }
      }
    }
    if ($tree.Count -eq 0) { Say '  Процессов Rhizomind нет.'; break }
    if ($deadline -and (Get-Date) -gt $deadline) { break }
    if ([Console]::KeyAvailable) { [void][Console]::ReadKey($true); break }
    Start-Sleep -Seconds 2
  }
  $found = @($script:LiveFindings | Where-Object { $_.Phase -eq $phase })
  if ($found.Count -eq 0) {
    Good "За этап «$phase» соединений процессов Rhizomind не замечено."
    Note "- Опрос соединений (этап «$phase»): процессы Rhizomind соединений не открывали."
  } else {
    Note "- Опрос соединений (этап «$phase»): **найдено $($found.Count)** — см. таблицу в конце."
  }
}

# --- Шаг 1. Подготовка -------------------------------------------------------

Set-Content -LiteralPath $Results -Encoding UTF8 -Value @"
# Сетевой аудит Rhizomind — результаты

Дата: $(Get-Date -Format 'yyyy-MM-dd HH:mm'). Компьютер: $env:COMPUTERNAME (имя пользователя не записывается).
Инструкция: docs/security/network-audit.md.
"@

Stage 'Подготовка'
Say 'Скрипт проверит машину и запишет условия аудита.'
Say 'Перед началом закройте все программы, кроме этого окна (браузер, Teams, Outlook, OneDrive можно оставить, но тогда их обращения попадут в общий журнал).'
Say ''

while (-not $AppExe -or -not (Test-Path -LiteralPath $AppExe)) {
  $AppExe = Ask 'Полный путь к rhizomind.exe (портативная сборка или установленная)'
}
while (-not $Procmon -or -not (Test-Path -LiteralPath $Procmon)) {
  $Procmon = Ask 'Полный путь к Procmon64.exe (Sysinternals Process Monitor)'
}

$os = Get-CimInstance Win32_OperatingSystem
$wv = $null
foreach ($k in @(
    'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
    'HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
    'HKCU:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}')) {
  if (-not $wv) { $wv = (Get-ItemProperty -LiteralPath $k -ErrorAction SilentlyContinue).pv }
}
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
$wlid = Get-Service -Name wlidsvc -ErrorAction SilentlyContinue
$appHash = (Get-FileHash -LiteralPath $AppExe -Algorithm SHA256).Hash

Note "- Windows: $($os.Caption) $($os.Version), сборка $($os.BuildNumber)"
Note "- WebView2 Runtime: $wv"
Note "- rhizomind.exe: SHA-256 $appHash"
Note "- Запуск от администратора: $isAdmin"
if ($wlid) { Note "- Служба wlidsvc: $($wlid.Status), запуск $($wlid.StartType)" }

$policyRoots = @('HKLM:\SOFTWARE\Policies\Microsoft\Edge\WebView2', 'HKCU:\SOFTWARE\Policies\Microsoft\Edge\WebView2',
  'HKLM:\SOFTWARE\Policies\Microsoft\Edge', 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\DataCollection',
  'HKLM:\SOFTWARE\Policies\Microsoft\Windows\Windows Error Reporting')
Note '- Политики (если пусто — не заданы):'
foreach ($r in $policyRoots) {
  if (Test-Path -LiteralPath $r) {
    $props = Get-ItemProperty -LiteralPath $r
    $names = @($props.PSObject.Properties | Where-Object { $_.Name -notlike 'PS*' } | ForEach-Object { "$($_.Name)=$($_.Value)" })
    Note ("  - ``{0}``: {1}" -f $r, ($names -join '; '))
    Say ("Политики в {0}: {1}" -f $r, ($names -join '; '))
  }
}
Say ''
Say "Windows: $($os.Caption) $($os.BuildNumber); WebView2: $wv; администратор: $isAdmin"
if (-not $wv) { Bad 'WebView2 Runtime не найден — Rhizomind не запустится.' }
if (-not $isAdmin) { Say 'Без прав администратора журнал DNS не включить: проверка DNS будет только по Process Monitor.' }
Wait-Enter

# --- Шаг 2. Снимок «до» ------------------------------------------------------

Stage 'Снимок до запуска'
$before = @{
  temp       = @(Get-ChildItem -LiteralPath $env:TEMP -Directory -Filter 'rhizomind-webview-*' -ErrorAction SilentlyContinue | ForEach-Object Name)
  profile    = @(Get-ChildItem -LiteralPath (Join-Path $env:LOCALAPPDATA 'Rhizomind') -ErrorAction SilentlyContinue | ForEach-Object Name)
  tokens     = @(Get-ChildItem -LiteralPath (Join-Path $env:LOCALAPPDATA 'Microsoft\TokenBroker\Cache') -ErrorAction SilentlyContinue | ForEach-Object { "$($_.Name) $($_.LastWriteTimeUtc.ToString('o'))" })
  wer        = @(Get-ChildItem -LiteralPath "$env:ProgramData\Microsoft\Windows\WER\ReportQueue", "$env:LOCALAPPDATA\Microsoft\Windows\WER\ReportQueue" -ErrorAction SilentlyContinue | ForEach-Object Name)
}
Note "- Остатки временных папок движка в %TEMP%: $($before.temp.Count)"
Note "- Файлы в %LOCALAPPDATA%\Rhizomind: $($before.profile -join ', ')"
Note "- Файлов в TokenBroker\Cache: $($before.tokens.Count)"
Good 'Снимок сделан.'

$dnsWasEnabled = $null
if ($isAdmin -and (Confirm 'Включить журнал DNS-клиента Windows на время аудита (после аудита вернётся как было)?')) {
  $dnsWasEnabled = ((wevtutil gl 'Microsoft-Windows-DNS-Client/Operational') -match 'enabled: true').Count -gt 0
  wevtutil sl 'Microsoft-Windows-DNS-Client/Operational' /e:true | Out-Null
  Good 'Журнал DNS включён.'
}
Wait-Enter

# --- Шаг 3. Process Monitor --------------------------------------------------

Stage 'Запуск Process Monitor'
Say 'Process Monitor будет записывать все события машины в файл capture.pml в папке результатов.'
Say 'Окно Process Monitor может появиться свёрнутым — не закрывайте его до шага 10.'
Start-Process -FilePath $Procmon -ArgumentList @('/AcceptEula', '/Quiet', '/Minimized', '/BackingFile', "`"$Pml`"")
Start-Sleep -Seconds 5
$captureStart = Get-Date
Note "- Запись Process Monitor начата: $($captureStart.ToString('HH:mm:ss'))"
Good 'Запись идёт.'
Wait-Enter

# --- Шаг 4. Запуск Rhizomind и флаги движка ----------------------------------

Stage 'Запуск Rhizomind и проверка флагов движка'
Step 'Скрипт запустит Rhizomind. Окно может появиться через 1–3 секунды.'
$proc = Start-Process -FilePath $AppExe -PassThru
$script:RootPid = $proc.Id
Start-Sleep -Seconds 6
$tree = Get-AppTree
$browser = $tree | Where-Object { $_.Name -ieq 'msedgewebview2.exe' -and $_.CommandLine -notmatch '--type=' } | Select-Object -First 1
if (-not $browser) {
  Bad 'Процесс браузера WebView2 не найден. Открылось ли окно Rhizomind? Подождите и нажмите Enter для повторной проверки.'
  Wait-Enter
  $tree = Get-AppTree
  $browser = $tree | Where-Object { $_.Name -ieq 'msedgewebview2.exe' -and $_.CommandLine -notmatch '--type=' } | Select-Object -First 1
}
Note "- Процессов Rhizomind и движка: $($tree.Count)"
$expected = @(
  @{ flag = '--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection'; must = $true },
  @{ flag = '--no-proxy-server'; must = $true },
  @{ flag = '--disable-background-networking'; must = $true },
  @{ flag = '--disable-component-update'; must = $true },
  @{ flag = '--disable-domain-reliability'; must = $true },
  @{ flag = '--no-pings'; must = $true },
  @{ flag = '--host-resolver-rules='; must = $true },
  @{ flag = '--edge-webview-disable-crash-reporting=1'; must = $true },
  @{ flag = '--remote-debugging-port'; must = $false }
)
if ($browser) {
  $cmd = [string]$browser.CommandLine
  Note ''
  Note '| Флаг | Ожидается | В командной строке процесса браузера |'
  Note '|---|---|---|'
  foreach ($e in $expected) {
    $has = $cmd.Contains($e.flag)
    $ok = ($has -eq $e.must)
    $want = 'есть'; if (-not $e.must) { $want = 'нет' }
    $got = 'нет'; if ($has) { $got = 'есть' }
    Note ("| ``{0}`` | {1} | {2}{3} |" -f $e.flag, $want, $got, $(if ($ok) { '' } else { ' — **нарушение**' }))
    if ($ok) { Good "$($e.flag): $got" } else { Bad "$($e.flag): $got (ожидалось: $want)" }
  }
  $udd = [regex]::Match($cmd, '--user-data-dir="?([^"]+)"?').Groups[1].Value
  $inTemp = $udd -like "*rhizomind-webview-*"
  Note ''
  Note "- Папка данных движка во временной папке ``rhizomind-webview-*``: $inTemp"
  if ($inTemp) { Good "Папка данных: $udd" } else { Bad "Папка данных не во временной папке: $udd" }
  $script:EngineDir = Split-Path -Parent $udd
} else {
  Note '- **Процесс браузера WebView2 не найден** — флаги не проверены.'
}
Wait-Enter

# --- Шаг 5. Простой ----------------------------------------------------------

Stage 'Простой: фоновые обращения движка'
Say 'Ничего не делайте в Rhizomind: окно открыто, программа простаивает.'
Say 'Здесь проявились бы фоновые службы движка: конфигурация и эксперименты, обновление компонентов, отчёты.'
$idle = [int](Ask 'Сколько минут ждать' '10')
Watch-Network 'простой' $idle

# --- Шаг 6. Сценарий: хранилище и заметки -------------------------------------

Stage 'Сценарий: хранилище и заметки'
Step 'Если открылось «Управление хранилищами» — создайте хранилище «Аудит» в новой папке на диске C: (кнопка «Обзор…»).'
Step 'Создайте 3 заметки и папку; в заметке наберите несколько абзацев, список, задачу - [ ], код в `обратных кавычках`.'
Step 'Вставьте вики-ссылку [[Вторая заметка]] и перейдите по ней; создайте заметку по неразрешённой ссылке.'
Step 'Ctrl+O — найдите заметку; Alt+← и Alt+→ — назад и вперёд; Ctrl+E — режим исходного текста и обратно.'
Step 'Переименуйте заметку в дереве (F2), перетащите в папку, удалите заметку (в корзину).'
Step 'Откройте папку хранилища в Проводнике, измените заметку Блокнотом и сохраните — вернитесь в Rhizomind.'
Watch-Network 'хранилище и заметки'
$vaultPath = Ask 'Полный путь к папке хранилища (для разбора журнала; можно пропустить — возьмётся из vaults.json)'

# --- Шаг 7. Сценарий: поля ввода, меню, настройки -----------------------------

Stage 'Сценарий: поля ввода, контекстное меню, настройки'
Step 'Наберите слова с ошибками («превет», «малако») в заметке, в поиске Ctrl+O, в поле переименования, в поиске горячих клавиш в настройках.'
Step 'Откройте «Настройки хранилища»: смените тему, ширину текста, размер шрифта; назначьте горячую клавишу; закройте настройки.'
Step 'Щёлкните правой кнопкой: в тексте заметки, на выделенном слове, на пустом месте окна. НИЧЕГО в меню движка не выбирайте — только перепишите пункты.'
Watch-Network 'поля, меню, настройки'
$menu = Ask 'Перечислите пункты контекстного меню в тексте заметки и на выделении (через запятую)'
Note "- Пункты контекстного меню движка: $menu"

# --- Шаг 8. Сценарий: внешние ссылки ------------------------------------------

Stage 'Сценарий: внешние ссылки'
Step 'В заметке наберите [Пример](https://example.com) и щёлкните по ссылке. В окне подтверждения нажмите «Отмена».'
Step 'Щёлкните ещё раз и нажмите «Открыть в браузере»: откроется системный браузер. Его соединения — норма, это действие пользователя; закройте браузер.'
Step 'Наберите [Почта](mailto:test@example.com), щёлкните — «Отмена».'
Step 'Наберите [Файл](file:///C:/Windows/win.ini) и щёлкните — ничего не должно открыться.'
Watch-Network 'внешние ссылки'
$cancelOk = Confirm 'После «Отмена» браузер не открывался?'
$fileOk = Confirm 'Ссылка file: ничего не открыла?'
Note "- «Отмена» не открыла браузер: $cancelOk; ссылка file: ничего не открыла: $fileOk"

# --- Шаг 9. Второе хранилище и выход ------------------------------------------

Stage 'Второе хранилище и выход'
Step 'Через переключатель слева внизу откройте «Управление хранилищами», создайте второе хранилище, откройте его.'
Step 'Наберите текст в заметке второго хранилища и сразу закройте окна крестиком «×» (все окна Rhizomind).'
Watch-Network 'второе хранилище и выход'
$waitUntil = (Get-Date).AddSeconds(30)
while ((Get-AppTree | Where-Object { Test-OwnImage $_.Name }).Count -gt 0 -and (Get-Date) -lt $waitUntil) { Start-Sleep -Seconds 1 }
Start-Sleep -Seconds 3
$left = @(Get-AppTree | Where-Object { Test-OwnImage $_.Name })
if ($left.Count -gt 0) { Bad "Процессы Rhizomind живы через 30 с после закрытия: $($left.Count)" } else { Good 'Все процессы Rhizomind завершились.' }
Note "- Процессы Rhizomind после выхода: $($left.Count)"
$tempAfter = @(Get-ChildItem -LiteralPath $env:TEMP -Directory -Filter 'rhizomind-webview-*' -ErrorAction SilentlyContinue | ForEach-Object Name)
$newTemp = @($tempAfter | Where-Object { $before.temp -notcontains $_ })
if ($newTemp.Count -eq 0) { Good 'Временная папка движка удалена.' } else { Bad "Осталась временная папка: $($newTemp -join ', ')" }
Note "- Новые временные папки движка после выхода: $($newTemp.Count)"
$tokensAfter = @(Get-ChildItem -LiteralPath (Join-Path $env:LOCALAPPDATA 'Microsoft\TokenBroker\Cache') -ErrorAction SilentlyContinue | ForEach-Object { "$($_.Name) $($_.LastWriteTimeUtc.ToString('o'))" })
$tokenChanges = @($tokensAfter | Where-Object { $before.tokens -notcontains $_ })
Note "- Новые или изменённые файлы TokenBroker\Cache: $($tokenChanges.Count) $(if ($tokenChanges.Count) { '(' + ($tokenChanges -join '; ') + ')' })"
$werAfter = @(Get-ChildItem -LiteralPath "$env:ProgramData\Microsoft\Windows\WER\ReportQueue", "$env:LOCALAPPDATA\Microsoft\Windows\WER\ReportQueue" -ErrorAction SilentlyContinue | ForEach-Object Name)
$newWer = @($werAfter | Where-Object { $before.wer -notcontains $_ })
Note "- Новые отчёты WER в очереди: $($newWer.Count) $(if ($newWer.Count) { '(' + ($newWer -join '; ') + ')' })"

$vaultsJson = Join-Path $env:LOCALAPPDATA 'Rhizomind\vaults.json'
$vaultRoots = New-Object System.Collections.ArrayList
if ($vaultPath) { [void]$vaultRoots.Add($vaultPath) }
if (Test-Path -LiteralPath $vaultsJson) {
  $list = Get-Content -Raw -Encoding UTF8 -LiteralPath $vaultsJson | ConvertFrom-Json
  $fields = @($list.PSObject.Properties.Name)
  Note "- vaults.json: поля верхнего уровня $($fields -join ', '); хранилищ $(@($list.vaults).Count)"
  foreach ($v in @($list.vaults)) {
    [void]$vaultRoots.Add([string]$v.path)
    Note ("  - хранилище: поля {0}" -f (@($v.PSObject.Properties.Name) -join ', '))
  }
  Say ''
  Say 'Содержимое vaults.json:'
  Get-Content -Encoding UTF8 -LiteralPath $vaultsJson | Write-Host
  $noNotes = Confirm 'В vaults.json нет имён заметок и текста (только имена и пути хранилищ, размеры окна)?'
  Note "- В vaults.json нет имён и текста заметок (проверено глазами): $noNotes"
}
Wait-Enter

# --- Шаг 10. Остановить запись и разобрать журнал -----------------------------

Stage 'Остановка Process Monitor и разбор журнала'
Say 'Запись останавливается, журнал выгружается в CSV и разбирается. Это может занять 5–20 минут.'
Start-Process -FilePath $Procmon -ArgumentList '/Terminate' -Wait
Start-Sleep -Seconds 3
Start-Process -FilePath $Procmon -ArgumentList @('/AcceptEula', '/Quiet', '/Minimized', '/OpenLog', "`"$Pml`"", '/SaveAs', "`"$Csv`"") -Wait
if (-not (Test-Path -LiteralPath $Csv)) {
  Bad 'CSV не получился. Откройте capture.pml в Process Monitor вручную: File → Save… → формат CSV, все события, в эту папку как capture.csv. Затем нажмите Enter.'
  Wait-Enter
}

# Номера процессов приложения: корень и всё, что он и его потомки
# (rhizomind.exe, msedgewebview2.exe) запускали, — по событиям Process Create.
$own = New-Object 'System.Collections.Generic.HashSet[int]'
[void]$own.Add([int]$script:RootPid)
foreach ($k in $script:Seen.Keys) { if (Test-OwnImage $script:Seen[$k].Name) { [void]$own.Add([int]$k) } }

$engineDir = $script:EngineDir
$allowedWrite = @()
foreach ($v in $vaultRoots) { if ($v) { $allowedWrite += $v.TrimEnd('\') + '\' } }
if ($engineDir) { $allowedWrite += $engineDir.TrimEnd('\') + '\' }
$allowedWrite += (Join-Path $env:LOCALAPPDATA 'Rhizomind') + '\'

$knownReg = @(
  @{ re = '\\Software\\Microsoft\\EdgeWebView(\\|$)'; why = 'движок WebView2, решение 61' },
  @{ re = '\\EdgeUpdate\\ClientState\\\{F3017226'; why = 'движок WebView2, решение 61' },
  @{ re = '\\Explorer\\ComDlg32\\'; why = 'окно выбора папки, задача 0.8' },
  @{ re = '\\CIDOpen\\|\\Explorer\\Modules\\'; why = 'окно выбора папки, задача 0.8' },
  @{ re = '\\Shell\\BagMRU|\\Shell\\Bags\\'; why = 'ShellBags окна выбора папки (локально)' },
  @{ re = '\\CapabilityAccessManager\\ConsentStore\\'; why = 'Windows, учтено (не проверено, аудит 3.3)' },
  @{ re = '\\Shell\\MuiCache'; why = 'оболочка Windows, решение 65' }
)
$knownFile = @(
  @{ re = '\\\$Recycle\.Bin\\'; why = 'корзина ОС (удаление заметки, инвариант 4)' },
  @{ re = '\\Microsoft\\TokenBroker\\'; why = 'WAM (OneAuth), решение 64 — должно отсутствовать' },
  @{ re = '\\Microsoft\\Credentials\\'; why = 'запись Windows' }
)
$writeOps = @('WriteFile', 'SetRenameInformationFile', 'SetDispositionInformationFile', 'SetEndOfFileInformationFile',
  'SetAllocationInformationFile', 'SetBasicInformationFile', 'CreateFile')
$regOps = @('RegSetValue', 'RegDeleteValue', 'RegDeleteKey', 'RegCreateKey')

$netOwn = @{}; $netAll = @{}; $fileOut = @{}; $regOut = @{}
$rows = 0
Import-Csv -LiteralPath $Csv | ForEach-Object {
  $rows++
  $op = $_.Operation
  $procId = 0; [void][int]::TryParse($_.PID, [ref]$procId)
  if ($op -eq 'Process Create' -and $own.Contains($procId)) {
    $m = [regex]::Match($_.Detail, 'PID: (\d+), Command line: (.*)')
    if ($m.Success -and $m.Groups[2].Value -match 'msedgewebview2\.exe|rhizomind\.exe') { [void]$own.Add([int]$m.Groups[1].Value) }
    return
  }
  if ($op -match '^(TCP|UDP) ') {
    $remote = ($_.Path -split ' -> ')[-1]
    if ($remote -match '^(localhost|127\.|\[?::1)') { return }
    $key = "$($_.'Process Name')`t$($op.Split(' ')[0])`t$remote"
    $netAll[$key] = 1 + [int]$netAll[$key]
    if ($own.Contains($procId)) { $netOwn[$key] = 1 + [int]$netOwn[$key] }
    return
  }
  if (-not $own.Contains($procId) -or $_.Result -ne 'SUCCESS') { return }
  if ($writeOps -contains $op) {
    if ($op -eq 'CreateFile' -and $_.Detail -notmatch 'OpenResult: (Created|Overwritten|Superseded)') { return }
    $p = $_.Path
    foreach ($a in $allowedWrite) { if ($p.StartsWith($a, [StringComparison]::OrdinalIgnoreCase)) { return } }
    if ($p -match '^\\Device\\|^C:\\\$|pipe\\') { if ($p -notmatch '\\\$Recycle\.Bin\\') { return } }
    $why = ''
    foreach ($k in $knownFile) { if ($p -match $k.re) { $why = $k.why; break } }
    $fileOut["$p`t$why"] = 1 + [int]$fileOut["$p`t$why"]
    return
  }
  if ($regOps -contains $op) {
    if ($op -eq 'RegCreateKey' -and $_.Detail -notmatch 'REG_CREATED_NEW_KEY') { return }
    $p = $_.Path
    $why = ''
    foreach ($k in $knownReg) { if ($p -match $k.re) { $why = $k.why; break } }
    $short = ($p -replace '\\[^\\]+$', '')
    $regOut["$short`t$why"] = 1 + [int]$regOut["$short`t$why"]
  }
}
Note "- Событий в журнале: $rows; процессов Rhizomind и движка: $($own.Count)"

function Write-Table([string] $title, [hashtable] $data, [string[]] $headers, [string] $file) {
  Note "`n### $title`n"
  if ($data.Count -eq 0) { Note 'Нет.'; return }
  Note ('| ' + ($headers -join ' | ') + ' | Событий |')
  Note ('|' + ('---|' * ($headers.Count + 1)))
  $objects = foreach ($k in ($data.Keys | Sort-Object)) {
    $parts = $k.Split("`t")
    Note ('| ' + (($parts | ForEach-Object { if ($_) { $_ } else { '—' } }) -join ' | ') + " | $($data[$k]) |")
    $o = [ordered]@{}
    for ($i = 0; $i -lt $headers.Count; $i++) { $o[$headers[$i]] = $parts[$i] }
    $o['Событий'] = $data[$k]
    [pscustomobject]$o
  }
  $objects | Export-Csv -LiteralPath (Join-Path $OutDir $file) -NoTypeInformation -Encoding UTF8
}

Write-Table 'Сеть процессов Rhizomind и движка (должно быть пусто)' $netOwn @('Процесс', 'Протокол', 'Адрес') 'summary-network-app.csv'
Write-Table 'Сеть всех процессов машины за время аудита (сверка со списком SECURITY.md)' $netAll @('Процесс', 'Протокол', 'Адрес') 'summary-network-all.csv'
Write-Table 'Записи файлов процессами Rhizomind вне хранилища, временной папки движка и %LOCALAPPDATA%\Rhizomind' $fileOut @('Путь', 'Учтено') 'summary-files.csv'
Write-Table 'Записи реестра процессами Rhizomind (по ключам)' $regOut @('Ключ', 'Учтено') 'summary-registry.csv'

if ($script:LiveFindings.Count -gt 0) {
  Note "`n### Соединения, замеченные опросом во время работы`n"
  Note '| Этап | Вид | Процесс | Номер | Адрес | Состояние |'
  Note '|---|---|---|---|---|---|'
  foreach ($f in $script:LiveFindings) { Note "| $($f.Phase) | $($f.Kind) | $($f.Process) | $($f.Pid) | $($f.Remote) | $($f.State) |" }
}

if ($null -ne $dnsWasEnabled) {
  $dns = @(Get-WinEvent -FilterHashtable @{ LogName = 'Microsoft-Windows-DNS-Client/Operational'; StartTime = $captureStart } -ErrorAction SilentlyContinue |
      Where-Object { $own.Contains([int]$_.ProcessId) })
  Note "`n### Запросы DNS процессов Rhizomind и движка (журнал DNS-клиента)`n"
  if ($dns.Count -eq 0) { Note 'Нет.' } else {
    $dns | ForEach-Object { [regex]::Match($_.Message, '(?i)(query|запрос)[^\r\n]*?([a-z0-9-]+\.)+[a-z]{2,}').Value } | Sort-Object -Unique | ForEach-Object { Note "- $_" }
  }
  if (-not $dnsWasEnabled) { wevtutil sl 'Microsoft-Windows-DNS-Client/Operational' /e:false | Out-Null }
}
Good 'Разбор закончен.'
Wait-Enter

# --- Шаг 11. Итог -------------------------------------------------------------

Stage 'Итог'
$appNet = $netOwn.Count + $script:LiveFindings.Count
if ($appNet -eq 0) {
  Good 'Процессы Rhizomind и движка не обращались к сети.'
  Note "`n**Итог по сети приложения:** обращений процессов Rhizomind и движка не найдено."
} else {
  Bad "Найдены сетевые обращения процессов Rhizomind или движка: $appNet. Это нарушение — пришлите результаты."
  Note "`n**Итог по сети приложения: найдено $appNet обращений — нарушение, разобрать.**"
}
Say ''
Say "Папка результатов: $OutDir"
Say 'Пришлите results.md и summary-*.csv. Файлы capture.pml и capture.csv не пересылайте:'
Say 'в них все события машины (имена файлов, пользователя). Удалите их сами, когда разбор закончен.'
Note "`nПримечания владельца:`n"
$extra = Ask 'Что ещё заметили (Enter — ничего)'
if ($extra) { Note $extra }

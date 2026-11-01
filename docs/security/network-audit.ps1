# Помощник ручного сетевого аудита Rhizomind (план 0.1, задачи 3.3 и 3.3б).
# Инструкция — docs/security/network-audit.md. Скрипт ведёт по шагам, сам
# снимает то, что можно снять командами, и собирает итог в results.md.
#
# Запуск — в обычном окне Windows PowerShell 5.1 или PowerShell 7 (не в ISE),
# «Запуск от имени администратора» (нужен Process Monitor и журнал DNS):
#   powershell -ExecutionPolicy Bypass -File network-audit.ps1 `
#     -AppExe "C:\...\rhizomind.exe" -Procmon "C:\Tools\Procmon64.exe"
# Без прав администратора — только опрос соединений:
#   powershell -ExecutionPolicy Bypass -File network-audit.ps1 -AppExe "..." -NoProcmon
#
# Ключи:
#   -AppExe         путь к rhizomind.exe (если не указан — скрипт спросит)
#   -Procmon        путь к Procmon64.exe (если не указан — скрипт спросит)
#   -ProcmonConfig  больше не используется (задача 3.3в): скрипт сам сбрасывает
#                   настройки Process Monitor на время аудита и возвращает их;
#                   ключ принимается и игнорируется — старые команды работают
#   -NoProcmon      без Process Monitor: шаги 3 и 10 пропускаются, остаётся
#                   опрос соединений (и журнал DNS, если есть права)
#   -OutDir         папка результатов; по умолчанию C:\rhizomind-audit-<дата>
#                   (не на рабочем столе: он часто синхронизируется OneDrive)
#   -Unattended     автоматический режим для проверки самого скрипта в CI:
#                   вопросов нет, сценарии 6–8 пропускаются, простой —
#                   -IdleSeconds секунд, окно Rhizomind скрипт закрывает сам.
#                   Это не аудит.
#   -LibraryOnly    только объявить функции (для тестов: tests/audit/)
#   -AnalyzeOnly    только разбор готового CSV Process Monitor, без аудита:
#                   -AnalyzeOnly -Csv <capture.csv> [-RootPid <n>] [-Vault <папка>]
#                   [-OutDir <папка>]. RootPid — номер rhizomind.exe; если не
#                   указан, берётся из results.md рядом с CSV (имя канала
#                   --mojo-named-platform-channel-pipe=<номер>.… в командной
#                   строке движка). Итог — в <папка CSV>\reanalysis-<дата>\.
#
# Скрипт ничего не скачивает и в сеть не ходит. Он читает процессы, сетевые
# соединения, реестр, журнал DNS, файлы в профиле и пишет в свою папку
# результатов. Следы вне папки результатов (честно):
#   - настройки Process Monitor (реестр HKCU\Software\Sysinternals\Process
#     Monitor) на время аудита сбрасываются: ключ сохраняется в
#     procmon-settings-backup.reg в папке результатов, удаляется, а в конце
#     (и при сбое, и при Ctrl+C) возвращается из копии как был; если его не
#     было — удаляется. Закрыли окно крестиком — верните сами:
#     reg import <папка результатов>\procmon-settings-backup.reg;
#   - Process Monitor устанавливает свой драйвер (служба PROCMON24, файл в
#     C:\Windows\System32\drivers) — так он работает всегда, это не Rhizomind;
#   - включение журнала DNS (по согласию, с правами администратора) меняет
#     системный журнал Microsoft-Windows-DNS-Client/Operational: в нём
#     остаются события за время аудита; включённость журнала возвращается как
#     была (в том числе при сбое скрипта), события скрипт не стирает;
#   - сам Windows PowerShell ведёт свои журналы (история команд PSReadLine и
#     т. п.) — как при любом запуске PowerShell.
# Скрипт не удаляет файлов (и свои capture.pml/capture.csv тоже — их удаляете
# вы после разбора). Журнал Process Monitor
# (capture.pml, capture.csv) содержит всё, что делала машина: его не
# пересылайте, пересылайте results.md и summary-*.csv.
param(
  [string] $AppExe = '',
  [string] $Procmon = '',
  [string] $ProcmonConfig = '',
  [switch] $NoProcmon,
  [string] $OutDir = '',
  [switch] $Unattended,
  [int] $IdleSeconds = 30,
  [switch] $LibraryOnly,
  [switch] $AnalyzeOnly,
  [string] $Csv = '',
  [int] $RootPid = 0,
  [string[]] $Vault = @()
)

$ErrorActionPreference = 'Stop'
$TotalStages = 11
$script:Stage = 0
$script:Results = ''

# Файл фонового задания Windows GAEvents в TokenBroker\Cache (решение 64,
# docs/SECURITY.md): пишется и без приложения — учтён.
$TokenBrokerBackgroundFile = '01c5cb21ab1d4fd56e65159d6c36cd5db1f647e1.tbres'

# Остановка аудита с понятным сообщением (ловится в конце, finally всё убирает).
function Stop-Audit([string] $why) { throw (New-Object System.OperationCanceledException $why) }

# --- Вывод и вопросы ---------------------------------------------------------

function Stage([string] $title) {
  $script:Stage++
  if (-not $Unattended) { try { Clear-Host } catch { $null = $_ } }
  Write-Host ('=' * 72) -ForegroundColor DarkCyan
  Write-Host ("Шаг {0} из {1}: {2}" -f $script:Stage, $TotalStages, $title) -ForegroundColor Cyan
  Write-Host ('=' * 72) -ForegroundColor DarkCyan
  Write-Host ''
  Add-Content -LiteralPath $script:Results -Encoding UTF8 -Value ("`n## Шаг {0}. {1}`n" -f $script:Stage, $title)
}

function Say([string] $text) { Write-Host $text }
function Step([string] $text) { Write-Host ('  > ' + $text) -ForegroundColor Yellow }
function Good([string] $text) { Write-Host ('  OK  ' + $text) -ForegroundColor Green }
function Bad([string] $text) { Write-Host ('  !!  ' + $text) -ForegroundColor Red }

function Note([string] $text) { Add-Content -LiteralPath $script:Results -Encoding UTF8 -Value $text }

function Wait-Enter([string] $text = 'Нажмите Enter, когда будет готово') {
  if ($Unattended) { return }
  [void](Read-Host ("`n" + $text))
}

function Ask([string] $question, [string] $default = '') {
  if ($Unattended) { return $default }
  $suffix = ''
  if ($default) { $suffix = " [$default]" }
  $answer = Read-Host ($question + $suffix)
  if (-not $answer) { $answer = $default }
  return $answer
}

# Ответ «да/нет». Принимает русскую и английскую раскладку: «д» на английской
# раскладке — «l». Одиночное «y» не принимается: это «н» в английской
# раскладке (принимается «yes»); «т» (n на русской раскладке) — «нет».
function ConvertTo-YesNo([string] $answer) {
  $a = ([string]$answer).Trim().ToLowerInvariant()
  if ($a -in @('д', 'да', 'l', 'yes', 'lf')) { return $true }
  if ($a -in @('н', 'нет', 'n', 'no', 'т', 'ytn')) { return $false }
  return $null
}

function Confirm([string] $question, [bool] $default = $false) {
  if ($Unattended) { return $default }
  while ($true) {
    $r = ConvertTo-YesNo (Read-Host ($question + ' (д/н)'))
    if ($null -ne $r) { return $r }
    Say '  Введите «д» (да) или «н» (нет).'
  }
}

# Путь, введённый человеком: без пробелов по краям и без кавычек, которые
# добавляет «Копировать как путь» в Проводнике.
function ConvertFrom-TypedPath([string] $text) {
  return ([string]$text).Trim().Trim('"').Trim("'").Trim()
}

# --- Пути: короткие имена 8.3 и полная форма ----------------------------------

# Полная форма пути: компоненты вида RUNNER~1 заменяются длинными именами
# (FindFirstFile по маске находит файл и по короткому имени). Путь должен
# существовать; иначе $null.
function Get-LongPath([string] $Path) {
  try {
    if (-not $Path -or -not (Test-Path -LiteralPath $Path)) { return $null }
    $full = [System.IO.Path]::GetFullPath($Path)
    $root = [System.IO.Path]::GetPathRoot($full)
    $parts = $full.Substring($root.Length).Split([char[]]@('\'), [StringSplitOptions]::RemoveEmptyEntries)
    $cur = $root
    foreach ($part in $parts) {
      $name = $part
      if ($part.Contains('~')) {
        $item = Get-ChildItem -LiteralPath $cur -Filter $part -Force -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($item) { $name = $item.Name }
      }
      $cur = Join-Path $cur $name
    }
    return $cur
  } catch { return $null }
}

# Короткая форма 8.3 (Scripting.FileSystemObject). Путь должен существовать.
function Get-ShortPath([string] $Path) {
  try {
    if (-not $Path -or -not (Test-Path -LiteralPath $Path)) { return $null }
    $fso = New-Object -ComObject Scripting.FileSystemObject
    if (Test-Path -LiteralPath $Path -PathType Container) { return $fso.GetFolder($Path).ShortPath }
    return $fso.GetFile($Path).ShortPath
  } catch { return $null }
}

# Все формы пути (как дан, полная, короткая) без завершающей «\».
function Get-PathForms([string] $Path) {
  $forms = New-Object System.Collections.Generic.List[string]
  $p = ConvertFrom-TypedPath $Path
  if (-not $p) { return @() }
  foreach ($f in @($p, (Get-LongPath $p), (Get-ShortPath $p))) {
    if (-not $f) { continue }
    $f = ([string]$f).TrimEnd('\')
    if (-not ($forms -contains $f)) { $forms.Add($f) }
  }
  return $forms.ToArray()
}

# Текст для results.md без имени пользователя: пути %TEMP%, %LOCALAPPDATA%,
# %USERPROFILE% (в полной и короткой форме) заменяются условными обозначениями.
$script:UserPathPairs = $null

function Protect-UserText([string] $text) {
  if (-not $text) { return $text }
  if ($null -eq $script:UserPathPairs) {
    $pairs = New-Object System.Collections.ArrayList
    foreach ($v in @(@('%TEMP%', $env:TEMP), @('%LOCALAPPDATA%', $env:LOCALAPPDATA), @('%APPDATA%', $env:APPDATA), @('%USERPROFILE%', $env:USERPROFILE))) {
      foreach ($f in (Get-PathForms $v[1])) { [void]$pairs.Add([pscustomobject]@{ Path = $f; Name = $v[0] }) }
    }
    $script:UserPathPairs = @($pairs | Sort-Object { $_.Path.Length } -Descending)
  }
  $out = $text
  foreach ($pair in $script:UserPathPairs) {
    $out = [regex]::Replace($out, [regex]::Escape($pair.Path), $pair.Name.Replace('$', '$$'), 'IgnoreCase')
  }
  if ($env:USERNAME) { $out = [regex]::Replace($out, [regex]::Escape($env:USERNAME), '<пользователь>', 'IgnoreCase') }
  return $out
}

# Папка под OneDrive или в перемещаемом профиле — синхронизируется.
function Test-SyncedPath([string] $Path) {
  $p = ([string]$Path).TrimEnd('\') + '\'
  foreach ($root in @($env:OneDrive, $env:OneDriveCommercial, $env:OneDriveConsumer, $env:APPDATA)) {
    if (-not $root) { continue }
    foreach ($f in (Get-PathForms $root)) {
      if ($p.StartsWith($f + '\', [StringComparison]::OrdinalIgnoreCase)) { return $true }
    }
  }
  return $false
}

# --- Снимки: TokenBroker и очередь WER ----------------------------------------

function Get-TokenCacheSnapshot {
  $map = @{}
  $dir = Join-Path $env:LOCALAPPDATA 'Microsoft\TokenBroker\Cache'
  Get-ChildItem -LiteralPath $dir -File -ErrorAction SilentlyContinue | ForEach-Object { $map[$_.Name] = $_.LastWriteTimeUtc.ToString('o') }
  return $map
}

# Изменения кэша брокера токенов: новый, изменённый или удалённый файл.
# Новый или изменённый 01c5cb21….tbres пишет фоновое задание Windows —
# учтено (решение 64); всё остальное, и удаление этого файла, — нарушение.
function Compare-TokenCache([hashtable] $Before, [hashtable] $After) {
  $changes = New-Object System.Collections.ArrayList
  foreach ($name in ($After.Keys | Sort-Object)) {
    $kind = $null
    if (-not $Before.ContainsKey($name)) { $kind = 'новый' } elseif ($Before[$name] -ne $After[$name]) { $kind = 'изменён' }
    if (-not $kind) { continue }
    $verdict = 'нарушение меры решения 64 — разобрать'
    if ($name -ieq $TokenBrokerBackgroundFile) { $verdict = 'фон Windows (задание GAEvents), учтено' }
    [void]$changes.Add([pscustomobject]@{ Name = $name; Change = $kind; Verdict = $verdict; Violation = ($name -ine $TokenBrokerBackgroundFile) })
  }
  foreach ($name in ($Before.Keys | Sort-Object)) {
    if ($After.ContainsKey($name)) { continue }
    [void]$changes.Add([pscustomobject]@{ Name = $name; Change = 'удалён'; Verdict = 'нарушение меры решения 64 — разобрать'; Violation = $true })
  }
  return , $changes.ToArray()
}

# Очередь отчётов WER. Системная очередь без прав администратора обычно
# недоступна — тогда так и пишется, а не «пусто».
function Get-WerQueue {
  $names = New-Object System.Collections.ArrayList
  $status = New-Object System.Collections.ArrayList
  $dirs = @(
    @('%ProgramData%\Microsoft\Windows\WER\ReportQueue', (Join-Path $env:ProgramData 'Microsoft\Windows\WER\ReportQueue')),
    @('%LOCALAPPDATA%\Microsoft\Windows\WER\ReportQueue', (Join-Path $env:LOCALAPPDATA 'Microsoft\Windows\WER\ReportQueue')))
  foreach ($d in $dirs) {
    try {
      if (-not (Test-Path -LiteralPath $d[1])) { [void]$status.Add("$($d[0]): папки нет (очередь пуста)"); continue }
      $items = @(Get-ChildItem -LiteralPath $d[1] -Force -ErrorAction Stop)
      foreach ($i in $items) { [void]$names.Add($i.Name) }
      [void]$status.Add("$($d[0]): доступна, записей $($items.Count)")
    } catch {
      [void]$status.Add("$($d[0]): **недоступна** (нет прав; запустите от администратора)")
    }
  }
  return [pscustomobject]@{ Names = $names.ToArray(); Status = $status.ToArray() }
}

# --- Журнал DNS ---------------------------------------------------------------

$DnsLogName = 'Microsoft-Windows-DNS-Client/Operational'

# Имена из событий DNS-клиента — из XML события (поле QueryName), а не из
# локализованного текста сообщения: так видны и имена без точки (wpad).
function Get-DnsQueryName([object[]] $Events) {
  $names = New-Object System.Collections.Generic.List[string]
  foreach ($e in $Events) {
    try {
      $xml = [xml]$e.ToXml()
      foreach ($d in @($xml.Event.EventData.Data)) {
        if ($d.Name -eq 'QueryName' -and $d.'#text' -and -not $names.Contains($d.'#text')) { $names.Add($d.'#text') }
      }
    } catch { $null = $_ }
  }
  return @($names | Sort-Object)
}

# --- Процессы Rhizomind ------------------------------------------------------

# Все когда-либо замеченные процессы приложения: номер -> имя и командная строка.
$script:Seen = @{}
$script:AnalyzeRootPid = $RootPid
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

# Нажата ли клавиша. В ISE и без консоли (CI) [Console]::KeyAvailable бросает
# исключение — тогда «не нажата».
function Test-KeyPressed {
  try {
    if ([Console]::KeyAvailable) { [void][Console]::ReadKey($true); return $true }
  } catch { $null = $_ }
  return $false
}

# Опрос сетевых соединений процессов приложения, пока не нажата клавиша
# (или не прошло $Seconds секунд).
$script:LiveFindings = New-Object System.Collections.ArrayList

function Watch-Network([string] $phase, [int] $Seconds = 0) {
  $deadline = $null
  if ($Seconds -gt 0) { $deadline = (Get-Date).AddSeconds($Seconds) }
  $keys = @{}
  Say ''
  if ($deadline) {
    Say ("Слежу за сетью процессов Rhizomind {0} с. Любая клавиша — закончить раньше." -f $Seconds)
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
    foreach ($p in $other) {
      $k = "OTHER $p"
      if (-not $keys.ContainsKey($k)) {
        $keys[$k] = $true
        Say ("  (запущен процесс не из Rhizomind: {0}, {1} — например, браузер по ссылке; его сеть не считается)" -f $script:Seen[$p].Name, $p)
      }
    }
    if ($tree.Count -eq 0) { Say '  Процессов Rhizomind нет.'; break }
    if ($deadline -and (Get-Date) -gt $deadline) { break }
    if (Test-KeyPressed) { break }
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

# --- Разбор журнала Process Monitor (шаг 10) ----------------------------------

# Службы Windows из docs/SECURITY.md, «Ожидаемые обращения к Microsoft».
$ExpectedServiceImages = @('svchost.exe', 'System', 'MicrosoftEdgeUpdate.exe', 'wermgr.exe', 'MsMpEng.exe', 'MpDefenderCoreService.exe')

# Оценка сетевого события: процесс Rhizomind или движка — нарушение; служба
# Windows из списка SECURITY.md — норма после сверки адреса; прочее — разобрать.
function Get-NetVerdict([string] $Process, [string] $Remote, [bool] $Own) {
  if ($Own) { return 'нарушение: процесс Rhizomind или движка' }
  $host_ = ($Remote -replace ':[^:]*$', '')
  if ($ExpectedServiceImages -contains $Process) {
    if ($host_ -match '^(20\.190\.|40\.126\.)|login\.live\.com|login\.microsoftonline\.com') {
      return 'служба входа Microsoft (wlidsvc, решение 64): при выходе программы не должна появляться — сверить время'
    }
    if ($host_ -match 'events\.data\.microsoft\.com') { return 'обязательная диагностика Windows (SECURITY.md) — норма' }
    if ($host_ -match 'ctldl\.windowsupdate\.com|crl\.microsoft\.com|^ocsp\.') { return 'проверка сертификатов (SECURITY.md) — норма' }
    return 'служба Windows из SECURITY.md «Ожидаемые обращения к Microsoft» — сверить адрес'
  }
  if ($Process -ieq 'msedgewebview2.exe') { return 'WebView2 другого приложения (не Rhizomind) — контроль для флагов' }
  return 'не из списка SECURITY.md — разобрать (фон машины?)'
}

$KnownReg = @(
  @{ re = '\\Software\\Microsoft\\EdgeWebView(\\|$)'; why = 'движок WebView2, решение 61' },
  @{ re = '\\EdgeUpdate\\ClientState\\\{F3017226'; why = 'движок WebView2, решение 61' },
  @{ re = '\\Explorer\\ComDlg32\\'; why = 'окно выбора папки, задача 0.8' },
  @{ re = '\\CIDOpen\\|\\Explorer\\Modules\\'; why = 'окно выбора папки, задача 0.8' },
  @{ re = '\\Shell\\BagMRU|\\Shell\\Bags\\'; why = 'ShellBags окна выбора папки (локально)' },
  @{ re = '\\CapabilityAccessManager\\ConsentStore\\'; why = 'Windows, решение 101 — принимается, если в ключе нет данных хранилища' },
  @{ re = '\\Shell\\MuiCache|\\Explorer\\UserAssist\\'; why = 'оболочка Windows, решение 65' },
  # Решение 117 (повторный аудит 01.11.2026): записи Windows и движка, SECURITY.md «Записи вне хранилища».
  @{ re = '\\Explorer\\BitBucket\\Volume\\\{[0-9a-f-]+\}(\\[^\\]+)?$'; why = 'корзина Windows: настройки тома, решение 117' },
  @{ re = '\\Internet Settings\\5\.0\\Cache\\(Content|Cookies|History)(\\[^\\]+)?$'; why = 'WinINet: настройки кэша, решение 117' },
  @{ re = '\\Internet Settings\\ZoneMap(\\[^\\]+)?$'; why = 'urlmon: зоны безопасности, решение 117' },
  @{ re = '\\Software\\Microsoft\\OneDrive\\Accounts(\\[^\\]+)?$'; why = 'расширение оболочки OneDrive, решение 117' },
  @{ re = '^HKLM\\System\\CurrentControlSet\\Services\\bam\\State\\UserSettings\\'; why = 'Windows BAM: запуск программы, решение 117' },
  @{ re = '^HKLM\\SOFTWARE\\WOW6432Node\\Microsoft\\EdgeUpdate\\ClientStateMedium\\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5\}(\\[^\\]+)?$'; why = 'движок WebView2, решение 117' },
  @{ re = '^HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Notifications\\Data(\\[^\\]+)?$'; why = 'платформа уведомлений Windows, решение 117' }
)
$KnownFile = @(
  @{ re = '\\\$Recycle\.Bin\\'; why = 'корзина ОС (удаление заметки, инвариант 4)' },
  @{ re = '\\Microsoft\\TokenBroker\\Cache\\01c5cb21ab1d4fd56e65159d6c36cd5db1f647e1\.tbres$'; why = 'фон Windows (задание GAEvents), учтено — решение 64' },
  @{ re = '\\Microsoft\\TokenBroker\\'; why = 'WAM (OneAuth), решение 64 — **должно отсутствовать**' },
  @{ re = '\\Microsoft\\Credentials\\'; why = 'запись Windows (SECURITY.md)' },
  # Решение 117 (повторный аудит 01.11.2026): кэш шейдеров Direct3D и драйвера Intel (процесс GPU движка).
  @{ re = '\\AppData\\Local\\D3DSCache\\'; why = 'кэш шейдеров Direct3D (движок), решение 117' },
  @{ re = '\\AppData\\LocalLow\\Intel\\ShaderCache\\'; why = 'кэш шейдеров драйвера Intel (движок), решение 117' },
  # Решение 118: файл учётной записи пишет библиотека OneAuth движка WebView2; другие файлы в OneAuth — «не учтено».
  @{ re = '\\Microsoft\\OneAuth\\accounts\\[0-9a-f]+$'; why = 'OneAuth движка WebView2, решение 118' }
)
$FileWriteOps = @('WriteFile', 'SetRenameInformationFile', 'SetDispositionInformationFile', 'SetEndOfFileInformationFile',
  'SetAllocationInformationFile', 'SetBasicInformationFile', 'CreateFile')
$RegWriteOps = @('RegSetValue', 'RegDeleteValue', 'RegDeleteKey', 'RegCreateKey')

function Add-Count([hashtable] $table, [string] $key) { $table[$key] = 1 + [int]$table[$key] }

# Класс события Process Monitor по столбцу Operation (как кнопки классов на
# панели Process Monitor): Registry, Network, Process, Profiling, остальное — File.
function Get-OperationClass([string] $Operation) {
  if ($Operation.StartsWith('Reg')) { return 'Registry' }
  if ($Operation.StartsWith('TCP ') -or $Operation.StartsWith('UDP ')) { return 'Network' }
  if ($Operation.EndsWith('Profiling')) { return 'Profiling' }
  if ($Operation.StartsWith('Process ') -or $Operation.StartsWith('Thread ') -or $Operation -eq 'Load Image') { return 'Process' }
  return 'File'
}

function New-ClassCounter { return [ordered]@{ File = 0; Registry = 0; Process = 0; Network = 0; Profiling = 0 } }

# Счётчики классов для results.md: «файлы 10, реестр 5, …».
function Format-ClassCounter($Counter) {
  return ('файлы {0}, реестр {1}, процессы {2}, сеть {3}, профилирование {4}' -f $Counter.File, $Counter.Registry, $Counter.Process, $Counter.Network, $Counter.Profiling)
}

# Столбцы, без которых разбор невозможен (CSV Process Monitor по умолчанию).
$ProcmonRequiredColumns = @('Process Name', 'PID', 'Operation', 'Path', 'Result', 'Detail')

function Get-CsvMissingColumn([string] $CsvPath) {
  $first = [string](Get-Content -LiteralPath $CsvPath -Encoding UTF8 -TotalCount 1)
  $cols = @($first.Split(',') | ForEach-Object { $_.Trim().Trim('"') })
  return @($ProcmonRequiredColumns | Where-Object { $cols -notcontains $_ })
}

# Разбирает CSV-выгрузку Process Monitor (столбцы по умолчанию: Time of Day,
# Process Name, PID, Operation, Path, Result, Detail).
#   OwnPids     — номера процессов Rhizomind и движка, известные заранее
#                 (корень и замеченные опросом); потомки добавляются по
#                 событиям Process Create;
#   VaultRoots  — папки хранилищ; EngineDir — временная папка движка
#                 (rhizomind-webview-…), в любой форме пути, в том числе 8.3;
#   AppDataDir  — %LOCALAPPDATA%\Rhizomind.
# Возвращает таблицы «ключ -> число событий»; ключ — поля через табуляцию.
function Invoke-ProcmonAnalysis {
  param(
    [Parameter(Mandatory = $true)] [string] $CsvPath,
    [int[]] $OwnPids = @(),
    [string[]] $VaultRoots = @(),
    [int] $RootPid = 0,
    [string] $EngineDir = '',
    [string] $AppDataDir = (Join-Path $env:LOCALAPPDATA 'Rhizomind')
  )
  $own = New-Object 'System.Collections.Generic.HashSet[int]'
  foreach ($p in $OwnPids) { if ($p) { [void]$own.Add([int]$p) } }
  # Корневой процесс (rhizomind.exe, запущенный скриптом): по умолчанию — первый
  # из OwnPids. Нет его событий в журнале — журнал ничего не доказывает.
  if (-not $RootPid -and @($OwnPids).Count -gt 0) { $RootPid = [int]@($OwnPids)[0] }

  # Разрешённые для записи префиксы — во всех формах пути (полная и 8.3).
  $allowed = New-Object System.Collections.Generic.List[string]
  foreach ($root in (@($VaultRoots) + @($AppDataDir) + @($EngineDir))) {
    foreach ($f in (Get-PathForms $root)) { if ($f -and -not $allowed.Contains($f + '\')) { $allowed.Add($f + '\') } }
  }
  # Временная папка движка узнаётся и по своему имени: оно уникально
  # (номер процесса и 16 случайных цифр), а в журнале путь может быть в другой
  # форме, чем в командной строке (8.3 у %TEMP% при кириллическом имени
  # пользователя), и к разбору папки уже нет — сравнить через диск нельзя.
  # Без -EngineDir (режим только разбора) имя берётся из командной строки
  # движка в событии Process Create процесса Rhizomind (--user-data-dir).
  $engineLeaf = ''
  if ($EngineDir) { $engineLeaf = '\' + (Split-Path -Leaf ((ConvertFrom-TypedPath $EngineDir).TrimEnd('\'))) }
  $longCache = @{}

  $r = [pscustomobject]@{
    Rows = 0; RootRows = 0; RootPid = $RootPid; Own = $own
    NetOwn = @{}; NetAll = @{}; FileOut = @{}; RegOut = @{}; Dictionaries = @{}
    DnsOwn = 0; DnsAll = 0
    Classes = (New-ClassCounter); RootClasses = (New-ClassCounter)
    TokenBrokerRows = 0; ConsentStoreRows = 0
    MissingColumns = @(Get-CsvMissingColumn $CsvPath); EngineLeaf = ''
  }

  Import-Csv -LiteralPath $CsvPath -Encoding UTF8 | ForEach-Object {
    $r.Rows++
    $op = [string]$_.Operation
    $procName = [string]$_.'Process Name'
    $procId = 0; [void][int]::TryParse([string]$_.PID, [ref]$procId)
    $class = Get-OperationClass $op
    $r.Classes[$class]++
    if ($RootPid -and $procId -eq $RootPid) { $r.RootRows++; $r.RootClasses[$class]++ }
    # Контроль полноты журнала (решение 107а): к этим путям обращаются
    # Windows и WAM; в неполном журнале их нет вовсе.
    $path_ = [string]$_.Path
    if ($path_.IndexOf('\TokenBroker\Cache', [StringComparison]::OrdinalIgnoreCase) -ge 0) { $r.TokenBrokerRows++ }
    elseif ($path_.IndexOf('\ConsentStore', [StringComparison]::OrdinalIgnoreCase) -ge 0) { $r.ConsentStoreRows++ }
    if ($op -eq 'Process Create' -and $own.Contains($procId)) {
      $m = [regex]::Match([string]$_.Detail, 'PID: (\d+), Command line: (.*)')
      if ($m.Success -and $m.Groups[2].Value -match 'msedgewebview2\.exe|rhizomind\.exe') {
        [void]$own.Add([int]$m.Groups[1].Value)
        if (-not $engineLeaf) {
          $udd = [regex]::Match($m.Groups[2].Value, '--user-data-dir="?([^"]+?)\\EBWebView')
          if ($udd.Success) {
            $leaf = Split-Path -Leaf $udd.Groups[1].Value
            if ($leaf -like 'rhizomind-webview-*') { $engineLeaf = '\' + $leaf }
          }
        }
      }
      return
    }
    if ($op -match '^(TCP|UDP) ') {
      $remote = (([string]$_.Path) -split ' -> ')[-1].Trim()
      if ($remote -match '^(localhost|127\.|\[?::1\]?:)') { return }
      $isOwn = $own.Contains($procId)
      if ($op -eq 'UDP Send' -and $remote -match ':(53|domain)$') {
        $r.DnsAll++
        if ($isOwn) { $r.DnsOwn++ }
      }
      $proto = $op.Split(' ')[0]
      Add-Count $r.NetAll ("$procName`t$proto`t$remote`t" + (Get-NetVerdict $procName $remote $isOwn))
      if ($isOwn) { Add-Count $r.NetOwn "$procName`t$proto`t$remote" }
      return
    }
    if (-not $own.Contains($procId) -or $_.Result -ne 'SUCCESS') { return }
    if ($FileWriteOps -contains $op) {
      if ($op -eq 'CreateFile' -and [string]$_.Detail -notmatch 'OpenResult: (Created|Overwritten|Superseded)') { return }
      $p = [string]$_.Path
      # Словари Hunspell проверки орфографии (скачиваются по сети) — отдельная
      # находка, где бы ни лежали, в том числе во временной папке движка.
      if ($p -match '\.bdic$|\\Dictionaries\\') { Add-Count $r.Dictionaries $p }
      if ($p.Contains('~')) {
        if (-not $longCache.ContainsKey($p)) { $longCache[$p] = Get-LongPath $p }
        if ($longCache[$p]) { $p = $longCache[$p] }
      }
      # Сама папка хранилища (её создаёт «Создать хранилище») — тоже своя запись:
      # в $allowed корни с «\» на конце, путь папки без него (решение 117).
      foreach ($a in $allowed) {
        if ($p.StartsWith($a, [StringComparison]::OrdinalIgnoreCase) -or $p.Equals($a.TrimEnd('\'), [StringComparison]::OrdinalIgnoreCase)) { return }
      }
      if ($engineLeaf -and ($p.IndexOf($engineLeaf + '\', [StringComparison]::OrdinalIgnoreCase) -ge 0 -or $p.EndsWith($engineLeaf, [StringComparison]::OrdinalIgnoreCase))) { return }
      if ($p -match '^\\Device\\|^[A-Z]:\\\$|pipe\\') { if ($p -notmatch '\\\$Recycle\.Bin\\') { return } }
      $why = '**не учтено**'
      foreach ($k in $KnownFile) { if ($p -match $k.re) { $why = $k.why; break } }
      Add-Count $r.FileOut "$p`t$why"
      return
    }
    if ($RegWriteOps -contains $op) {
      if ($op -eq 'RegCreateKey' -and [string]$_.Detail -notmatch 'REG_CREATED_NEW_KEY') { return }
      $p = [string]$_.Path
      $why = '**не учтено**'
      foreach ($k in $KnownReg) { if ($p -match $k.re) { $why = $k.why; break } }
      $short = ($p -replace '\\[^\\]+$', '')
      Add-Count $r.RegOut "$short`t$why"
    }
  }
  $r.EngineLeaf = $engineLeaf
  return $r
}

# Почему журнал Process Monitor ничего не доказывает — конкретные причины
# для итога (решение 107а). Пусто — журнал полный.
function Get-ProcmonGap([object] $Analysis) {
  if ($null -eq $Analysis) { return @('журнал не выгружен в CSV (файла capture.csv нет)') }
  $gaps = New-Object System.Collections.Generic.List[string]
  if (@($Analysis.MissingColumns).Count -gt 0) { $gaps.Add("в CSV нет столбцов $(@($Analysis.MissingColumns) -join ', ') (столбцы Process Monitor изменены)") }
  if ($Analysis.Rows -eq 0) { $gaps.Add('CSV пуст: в журнале нет событий'); return $gaps.ToArray() }
  if (-not $Analysis.RootPid) {
    $gaps.Add('не известен номер корневого процесса Rhizomind (RootPid)')
  } elseif ($Analysis.RootRows -eq 0) {
    $gaps.Add("нет ни одного события корневого процесса Rhizomind (PID $($Analysis.RootPid)): Process Monitor их отбросил (фильтр, «Drop Filtered Events») или журнал снят не с этого запуска")
  }
  if ($Analysis.Classes.File -eq 0) { $gaps.Add('нет файловых событий ни у одного процесса (класс File выключен?)') }
  if ($Analysis.Classes.Registry -eq 0) { $gaps.Add('нет событий реестра ни у одного процесса (класс Registry выключен?)') }
  if ($Analysis.Classes.Process -eq 0) { $gaps.Add('нет событий процессов ни у одного процесса (класс Process выключен?) — потомков Rhizomind не найти') }
  if ($Analysis.Classes.Network -eq 0) { $gaps.Add('нет сетевых событий ни у одного процесса (класс Network выключен?)') }
  return $gaps.ToArray()
}

# Что доказывает журнал Process Monitor для итога:
#   no-procmon — запуск с -NoProcmon (итог только по опросу соединений);
#   not-proven — журнал не разобран (CSV нет) или неполный (Get-ProcmonGap:
#                нет событий корневого процесса Rhizomind, нет класса событий);
#   analysed   — разобран и полный.
function Get-ProcmonVerdict([switch] $NoProcmon, [object] $Analysis) {
  if ($NoProcmon) { return 'no-procmon' }
  if (@(Get-ProcmonGap $Analysis).Count -gt 0) { return 'not-proven' }
  return 'analysed'
}

# --- Пробная запись Process Monitor (шаг 3, решение 107а) ---------------------

# Проверяет CSV пробной записи: есть ли события тестового процесса (cmd.exe,
# запущенного скриптом) классов File, Registry и Process. Тестовый процесс —
# по номеру (ProbePid) и по метке в командной строке (Process Start у него,
# Process Create у родителя).
function Test-ProcmonProbe {
  param(
    [Parameter(Mandatory = $true)] [string] $CsvPath,
    [int] $ProbePid = 0,
    [string] $Marker = 'procmon-probe'
  )
  $res = [pscustomobject]@{ Ok = $false; Rows = 0; ProbeRows = 0; Pids = @(); Classes = (New-ClassCounter); AllClasses = (New-ClassCounter); Reason = '' }
  if (-not (Test-Path -LiteralPath $CsvPath)) { $res.Reason = 'пробная запись не выгрузилась в CSV'; return $res }
  $rows = @(Import-Csv -LiteralPath $CsvPath -Encoding UTF8)
  $res.Rows = $rows.Count
  $pids = New-Object 'System.Collections.Generic.HashSet[int]'
  if ($ProbePid) { [void]$pids.Add($ProbePid) }
  foreach ($row in $rows) {
    $op = [string]$row.Operation
    $detail = [string]$row.Detail
    if ($op -eq 'Process Create') {
      $m = [regex]::Match($detail, 'PID: (\d+), Command line: (.*)')
      if ($m.Success -and $m.Groups[2].Value.Contains($Marker)) { [void]$pids.Add([int]$m.Groups[1].Value) }
    } elseif ($op -eq 'Process Start' -and $detail.Contains($Marker)) {
      $n = 0; if ([int]::TryParse([string]$row.PID, [ref]$n)) { [void]$pids.Add($n) }
    }
  }
  foreach ($row in $rows) {
    $class = Get-OperationClass ([string]$row.Operation)
    $res.AllClasses[$class]++
    $n = 0; [void][int]::TryParse([string]$row.PID, [ref]$n)
    if ($pids.Contains($n)) { $res.ProbeRows++; $res.Classes[$class]++ }
  }
  $res.Pids = @($pids)
  if ($res.Rows -eq 0) {
    $res.Reason = 'пробный журнал пуст: Process Monitor ничего не записал'
  } elseif ($res.ProbeRows -eq 0) {
    $res.Reason = "в пробном журнале нет ни одного события тестового процесса cmd.exe, хотя событий других процессов $($res.Rows): Process Monitor отбрасывает события"
  } else {
    $missing = @(@('File', 'Registry', 'Process') | Where-Object { $res.Classes[$_] -eq 0 })
    if ($missing.Count -gt 0) {
      $res.Reason = "у тестового процесса нет событий классов $($missing -join ', ') (классы событий Process Monitor выключены?)"
    } else {
      $res.Ok = $true
    }
  }
  return $res
}

# Номер корневого процесса Rhizomind из results.md прошлого прогона: движок
# получает его в имени канала --mojo-named-platform-channel-pipe=<номер>.<…>.
function Get-RootPidFromResult([string] $Text) {
  $m = [regex]::Match([string]$Text, '--mojo-named-platform-channel-pipe=(\d+)\.')
  if ($m.Success) { return [int]$m.Groups[1].Value }
  return 0
}

# --- Настройки Process Monitor: сброс на время аудита (решение 107а) ----------

$ProcmonKey = 'HKCU\Software\Sysinternals\Process Monitor'
$ProcmonKeyPs = 'HKCU:\Software\Sysinternals\Process Monitor'
$script:ProcmonSettingsReset = $false
$script:ProcmonKeyExisted = $false
$script:ProcmonBackup = ''

# Сохраняет ключ настроек Process Monitor в .reg и удаляет его: Process Monitor
# стартует с настройками по умолчанию (без фильтра и его правила «Drop Filtered
# Events», со всеми классами событий, без кольцевого буфера, с глубиной истории по умолчанию,
# со столбцами CSV по умолчанию). Показ имён вместо адресов выключается.
function Reset-ProcmonSetting([string] $BackupPath) {
  $script:ProcmonBackup = $BackupPath
  $script:ProcmonKeyExisted = Test-Path -LiteralPath $ProcmonKeyPs
  if ($script:ProcmonKeyExisted) {
    $p = Start-Process -FilePath 'reg.exe' -ArgumentList @('export', "`"$ProcmonKey`"", "`"$BackupPath`"", '/y') -Wait -PassThru -WindowStyle Hidden
    if ($p.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $BackupPath)) {
      Stop-Audit "Не удалось сохранить настройки Process Monitor (reg export, код $($p.ExitCode)) — без копии скрипт их не трогает."
    }
  }
  $script:ProcmonSettingsReset = $true
  if ($script:ProcmonKeyExisted) { Remove-Item -LiteralPath $ProcmonKeyPs -Recurse -Force }
  New-Item -Path $ProcmonKeyPs -Force | Out-Null
  New-ItemProperty -LiteralPath $ProcmonKeyPs -Name 'ResolveAddresses' -PropertyType DWord -Value 0 -Force | Out-Null
}

# Возвращает ключ как был (после того как все Process Monitor закрылись: при
# выходе он пишет свои настройки в этот ключ).
function Restore-ProcmonSetting {
  if (-not $script:ProcmonSettingsReset) { return }
  $until = (Get-Date).AddSeconds(30)
  while ((Get-Process -Name 'Procmon', 'Procmon64', 'Procmon64a' -ErrorAction SilentlyContinue) -and (Get-Date) -lt $until) { Start-Sleep -Seconds 1 }
  if (Test-Path -LiteralPath $ProcmonKeyPs) { Remove-Item -LiteralPath $ProcmonKeyPs -Recurse -Force }
  if ($script:ProcmonKeyExisted) {
    $p = Start-Process -FilePath 'reg.exe' -ArgumentList @('import', "`"$($script:ProcmonBackup)`"") -Wait -PassThru -WindowStyle Hidden
    if ($p.ExitCode -ne 0) { throw "reg import $($script:ProcmonBackup): код $($p.ExitCode)" }
  }
  $script:ProcmonSettingsReset = $false
}

function Write-Table([string] $title, [hashtable] $data, [string[]] $headers, [string] $file) {
  Note "`n### $title`n"
  if ($data.Count -eq 0) { Note 'Нет.'; return }
  Note ('| ' + ($headers -join ' | ') + ' | Событий |')
  Note ('|' + ('---|' * ($headers.Count + 1)))
  $objects = foreach ($k in ($data.Keys | Sort-Object)) {
    $parts = $k.Split("`t")
    Note ('| ' + (($parts | ForEach-Object { if ($_) { (Protect-UserText $_) } else { '—' } }) -join ' | ') + " | $($data[$k]) |")
    $o = [ordered]@{}
    for ($i = 0; $i -lt $headers.Count; $i++) { $o[$headers[$i]] = (Protect-UserText $parts[$i]) }
    $o['Событий'] = $data[$k]
    [pscustomobject]$o
  }
  $objects | Export-Csv -LiteralPath (Join-Path $OutDir $file) -NoTypeInformation -Encoding UTF8
}

# Раздел results.md с разбором журнала: полнота (классы событий, причины
# «не доказано») и таблицы. Общий для шага 10 и режима -AnalyzeOnly.
function Write-ProcmonReport([object] $Analysis) {
  Note "- Событий в журнале: $($Analysis.Rows); из них корневого процесса Rhizomind (PID $($Analysis.RootPid)): $($Analysis.RootRows); процессов Rhizomind и движка: $($Analysis.Own.Count)"
  Note "- Классы событий в журнале (все процессы): $(Format-ClassCounter $Analysis.Classes)"
  Note "- Классы событий корневого процесса Rhizomind: $(Format-ClassCounter $Analysis.RootClasses)"
  Note "- Контроль полноты (любые процессы): событий на путях ``TokenBroker\Cache`` — $($Analysis.TokenBrokerRows), ``ConsentStore`` — $($Analysis.ConsentStoreRows)"
  $gaps = @(Get-ProcmonGap $Analysis)
  if ($gaps.Count -gt 0) {
    Note "- **Журнал неполный — ничего не доказывает:** $($gaps -join '; ')."
    foreach ($g in $gaps) { Bad "Журнал неполный: $g" }
  } else {
    Good 'Журнал полный: есть события Rhizomind и все классы событий (файлы, реестр, процессы, сеть).'
  }
  Write-Table 'Сеть процессов Rhizomind и движка (должно быть пусто)' $Analysis.NetOwn @('Процесс', 'Протокол', 'Адрес') 'summary-network-app.csv'
  Note "`n- Запросы DNS (UDP Send на порт 53) процессов Rhizomind и движка: $($Analysis.DnsOwn) (должно быть 0 — подтверждение ``--host-resolver-rules``); всех процессов машины: $($Analysis.DnsAll)"
  Write-Table 'Сеть всех процессов машины за время аудита (сверка со списком SECURITY.md)' $Analysis.NetAll @('Процесс', 'Протокол', 'Адрес', 'Оценка') 'summary-network-all.csv'
  Write-Table 'Записи файлов процессами Rhizomind вне хранилища, временной папки движка и %LOCALAPPDATA%\Rhizomind' $Analysis.FileOut @('Путь', 'Учтено') 'summary-files.csv'
  Write-Table 'Словари проверки орфографии (Hunspell) — должно быть пусто' $Analysis.Dictionaries @('Путь') 'summary-dictionaries.csv'
  Write-Table 'Записи реестра процессами Rhizomind (по ключам)' $Analysis.RegOut @('Ключ', 'Учтено') 'summary-registry.csv'
}

# Итог по сети приложения. $LiveCount — находки опроса соединений; в режиме
# -AnalyzeOnly опроса нет.
function Write-NetVerdict([object] $Analysis, [int] $LiveCount, [bool] $NoPm, [bool] $WithPoll) {
  $appNet = $LiveCount
  if ($Analysis) { $appNet += $Analysis.NetOwn.Count }
  $pmVerdict = Get-ProcmonVerdict -NoProcmon:$NoPm -Analysis $Analysis
  $poll = ''
  if ($WithPoll) { $poll = ' Опрос соединений обращений не нашёл, но этого мало.' }
  if ($appNet -eq 0 -and $pmVerdict -eq 'not-proven') {
    $why = (@(Get-ProcmonGap $Analysis) -join '; ')
    Bad "Не доказано: $why."
    Note "`n**Итог по сети приложения:** **не доказано** — $why.$poll Аудит нужно повторить с рабочим журналом (инструкция, раздел «Если итог: не доказано»)."
  } elseif ($appNet -eq 0) {
    Good 'Процессы Rhizomind и движка не обращались к сети.'
    if ($NoPm) {
      Note "`n**Итог по сети приложения:** опрос соединений обращений не нашёл; журнал Process Monitor не снимался (-NoProcmon) — для критериев 3 и 4 этого недостаточно."
    } else {
      Note "`n**Итог по сети приложения:** обращений процессов Rhizomind и движка не найдено."
    }
  } else {
    Bad "Найдены сетевые обращения процессов Rhizomind или движка: $appNet. Это нарушение — пришлите результаты."
    Note "`n**Итог по сети приложения: найдено $appNet обращений — нарушение, разобрать.**"
  }
}

# Пути хранилищ из vaults.json этой машины (поле path).
function Get-VaultRootFromJson {
  $roots = New-Object System.Collections.ArrayList
  $vaultsJson = Join-Path $env:LOCALAPPDATA 'Rhizomind\vaults.json'
  if (Test-Path -LiteralPath $vaultsJson) {
    try {
      $list = Get-Content -Raw -Encoding UTF8 -LiteralPath $vaultsJson | ConvertFrom-Json
      foreach ($v in @($list.vaults)) { if ($v.path) { [void]$roots.Add([string]$v.path) } }
    } catch { $null = $_ }
  }
  return $roots.ToArray()
}

if ($LibraryOnly) { return }

# =============================================================================
# Режим только разбора (-AnalyzeOnly): готовый CSV Process Monitor -> итог в
# <папка CSV>\reanalysis-<дата>\results.md и summary-*.csv. Без Process Monitor,
# без Rhizomind и без вопросов. Код выхода: 0 — разбор сделан (итог — в
# results.md), 2 — нечего разбирать.
# =============================================================================

if ($AnalyzeOnly) {
  try { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8 } catch { $null = $_ }
  $code = 0
  try {
    $Csv = ConvertFrom-TypedPath $Csv
    if (-not $Csv -or -not (Test-Path -LiteralPath $Csv -PathType Leaf)) { Stop-Audit "CSV не найден: '$Csv'. Укажите -Csv <путь к capture.csv>." }
    $Csv = [System.IO.Path]::GetFullPath($Csv)
    $csvDir = Split-Path -Parent $Csv
    $pidSource = 'ключ -RootPid'
    $root = $script:AnalyzeRootPid
    $prev = Join-Path $csvDir 'results.md'
    if (-not $root -and (Test-Path -LiteralPath $prev)) {
      $root = Get-RootPidFromResult (Get-Content -Raw -Encoding UTF8 -LiteralPath $prev)
      $pidSource = 'results.md рядом с CSV (имя канала --mojo-named-platform-channel-pipe)'
    }
    if (-not $root) { Stop-Audit 'Не известен номер rhizomind.exe. Укажите -RootPid <номер> (число из --mojo-named-platform-channel-pipe=<номер>.… в results.md).' }
    if (-not $OutDir) { $OutDir = Join-Path $csvDir ('reanalysis-' + (Get-Date -Format 'yyyy-MM-dd-HHmm')) }
    $OutDir = [System.IO.Path]::GetFullPath((ConvertFrom-TypedPath $OutDir))
    if (Test-SyncedPath $OutDir) { Stop-Audit "Папка $OutDir — в OneDrive или в перемещаемом профиле. Укажите другую: -OutDir <путь>" }
    New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
    $script:Results = Join-Path $OutDir 'results.md'
    $roots = @(@($Vault | ForEach-Object { ConvertFrom-TypedPath $_ } | Where-Object { $_ }) + @(Get-VaultRootFromJson))
    Set-Content -LiteralPath $script:Results -Encoding UTF8 -Value @"
# Сетевой аудит Rhizomind — повторный разбор журнала Process Monitor

Дата разбора: $(Get-Date -Format 'yyyy-MM-dd HH:mm'). Режим -AnalyzeOnly: только разбор готового CSV, без нового аудита.
Инструкция: docs/security/network-audit.md.

- CSV: $(Protect-UserText $Csv)
- Корневой процесс Rhizomind: PID $root (источник: $pidSource)
- Папок хранилищ для разбора: $($roots.Count) (ключ -Vault и vaults.json этой машины)
"@
    Say "Разбор $Csv (PID rhizomind.exe $root). Большой CSV читается долго — минуты на миллион строк."
    $analysis = Invoke-ProcmonAnalysis -CsvPath $Csv -OwnPids @($root) -RootPid $root -VaultRoots $roots
    if ($analysis.EngineLeaf) { Note "- Временная папка движка (из командной строки в журнале): ``$($analysis.EngineLeaf.TrimStart('\'))``" } else { Note '- Временная папка движка в журнале не найдена — записи движка в неё попадут в «не учтено».' }
    Note "`n## Разбор журнала`n"
    Write-ProcmonReport $analysis
    Note "`n## Итог`n"
    Write-NetVerdict -Analysis $analysis -LiveCount 0 -NoPm $false -WithPoll $false
    Note '- Флаги движка, выход, TokenBroker, WER, журнал DNS — в results.md исходного аудита (этот режим их не проверяет).'
    Say "Итог: $script:Results"
  } catch [System.OperationCanceledException] {
    Bad $_.Exception.Message
    $code = 2
  } catch {
    Bad "Сбой разбора: $($_.Exception.Message) (строка $($_.InvocationInfo.ScriptLineNumber))"
    $code = 1
  }
  exit $code
}

# =============================================================================
# Сценарий. Всё — внутри try/finally: при любом сбое или Ctrl+C Process Monitor
# останавливается, журнал DNS возвращается как был. Закрытие окна PowerShell
# крестиком finally НЕ выполняет: тогда остановите Process Monitor сами
# (Procmon64.exe /Terminate) и верните журнал DNS (wevtutil sl … /e:false).
# =============================================================================

$script:ProcmonStarted = $false
$script:ProcmonStopped = $false
$script:DnsChanged = $false
$script:DnsWasEnabled = $false
$script:ExitCode = 0
$script:Completed = $false
$script:EngineDir = ''

function Stop-Procmon {
  if (-not $script:ProcmonStarted -or $script:ProcmonStopped) { return }
  Start-Process -FilePath $Procmon -ArgumentList '/Terminate' -Wait
  $script:ProcmonStopped = $true
}

function Get-ProcmonProcess { return @(Get-Process -Name 'Procmon', 'Procmon64', 'Procmon64a' -ErrorAction SilentlyContinue) }

# Запись в файл (/BackingFile — события пишутся в .pml, а не в память) без
# фильтра (/NoFilter — «Clear the filter at start up», справка procmon /?)
# поверх сброшенных настроек (Reset-ProcmonSetting). /WaitForIdle — дождаться,
# пока Process Monitor начнёт записывать.
function Start-ProcmonCapture([string] $BackingFile) {
  $pmArgs = @('/AcceptEula', '/Quiet', '/Minimized', '/NoFilter', '/BackingFile', "`"$BackingFile`"")
  try {
    Start-Process -FilePath $Procmon -ArgumentList $pmArgs
  } catch {
    Stop-Audit "Process Monitor не запустился ($($_.Exception.Message)). Без прав администратора запустите скрипт с -NoProcmon."
  }
  $script:ProcmonStarted = $true
  $script:ProcmonStopped = $false
  try {
    $w = Start-Process -FilePath $Procmon -ArgumentList @('/AcceptEula', '/WaitForIdle') -PassThru
    if ($w -and -not $w.WaitForExit(60000)) { Bad 'Process Monitor не сообщил о готовности за 60 с.' }
  } catch { $null = $_ }
  Start-Sleep -Seconds 2
  if ((Get-ProcmonProcess).Count -eq 0) {
    Bad 'Процесс Process Monitor не виден. Если запрос UAC был отклонён — запустите скрипт снова от администратора или с -NoProcmon.'
    if (-not (Confirm 'Process Monitor всё-таки записывает (окно открыто)?')) { Stop-Audit 'Process Monitor не запущен.' }
  }
}

# Выгрузка .pml в CSV всех событий: /SaveAs без /SaveApplyFilter, /NoFilter.
function Convert-ProcmonLog([string] $Pml, [string] $CsvPath) {
  Start-Process -FilePath $Procmon -ArgumentList @('/AcceptEula', '/Quiet', '/Minimized', '/NoFilter', '/OpenLog', "`"$Pml`"", '/SaveAs', "`"$CsvPath`"") -Wait
}

# Пробная запись: Process Monitor с теми же настройками пишет ~15 с, скрипт
# запускает тестовый процесс cmd.exe (читает реестр при старте, пишет файл
# procmon-probe.txt в папку результатов), запись останавливается, выгружается
# в CSV и проверяется (Test-ProcmonProbe). Основную запись так проверить
# нельзя: /Terminate останавливает все экземпляры Process Monitor, а открытый
# журнал идущей записи второй экземпляр не прочитает.
function Invoke-ProcmonProbe {
  $probePml = Join-Path $OutDir 'procmon-probe.pml'
  $probeCsv = Join-Path $OutDir 'procmon-probe.csv'
  $probeTxt = Join-Path $OutDir 'procmon-probe.txt'
  Start-ProcmonCapture $probePml
  Start-Sleep -Seconds 3
  $cmd = Join-Path $env:SystemRoot 'System32\cmd.exe'
  $p = Start-Process -FilePath $cmd -ArgumentList @('/c', "echo procmon-probe> `"$probeTxt`"") -PassThru -Wait -WindowStyle Hidden
  Start-Sleep -Seconds 5
  Stop-Procmon
  Start-Sleep -Seconds 2
  Convert-ProcmonLog $probePml $probeCsv
  return (Test-ProcmonProbe -CsvPath $probeCsv -ProbePid $p.Id)
}

if ($Unattended) {
  try { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8 } catch { $null = $_ }
}

try {
  if ($Host.Name -eq 'Windows PowerShell ISE Host') {
    Stop-Audit 'Скрипт не работает в PowerShell ISE (нет клавиши «закончить этап»). Запустите его в обычном окне PowerShell.'
  }

  # --- Папка результатов: не в OneDrive и не в перемещаемом профиле ---------
  if (-not $OutDir) {
    $OutDir = Join-Path ($env:SystemDrive + '\') ('rhizomind-audit-' + (Get-Date -Format 'yyyy-MM-dd-HHmm'))
  }
  $OutDir = [System.IO.Path]::GetFullPath((ConvertFrom-TypedPath $OutDir))
  if (Test-SyncedPath $OutDir) {
    Stop-Audit "Папка результатов $OutDir — в OneDrive или в перемещаемом профиле: журнал всех событий машины попал бы в синхронизацию. Укажите другую: -OutDir C:\rhizomind-audit"
  }
  try {
    New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
  } catch {
    Stop-Audit "Не удалось создать папку результатов $OutDir ($($_.Exception.Message)). Укажите другую: -OutDir <путь>"
  }
  $script:Results = Join-Path $OutDir 'results.md'
  $Pml = Join-Path $OutDir 'capture.pml'
  $Csv = Join-Path $OutDir 'capture.csv'

  # --- Шаг 1. Подготовка -----------------------------------------------------

  Set-Content -LiteralPath $script:Results -Encoding UTF8 -Value @"
# Сетевой аудит Rhizomind — результаты

Дата: $(Get-Date -Format 'yyyy-MM-dd HH:mm'). Компьютер: $env:COMPUTERNAME (имя пользователя не записывается).
Инструкция: docs/security/network-audit.md.
"@
  if ($Unattended) { Note "`n**Автоматический режим (-Unattended): проверка самого скрипта, не аудит.** Сценарии 6–8 пропущены, простой $IdleSeconds с." }
  if ($NoProcmon) { Note "`n**Без Process Monitor (-NoProcmon):** шаги 3 и 10 пропущены, сеть — только опросом соединений раз в 2 с (короткие обращения между опросами не видны)." }

  Stage 'Подготовка'
  Say 'Скрипт проверит машину и запишет условия аудита.'
  Say 'Перед началом закройте все программы, кроме этого окна (браузер, Teams, Outlook, OneDrive можно оставить, но тогда их обращения попадут в общий журнал).'
  Say ''

  $AppExe = ConvertFrom-TypedPath $AppExe
  while (-not $AppExe -or -not (Test-Path -LiteralPath $AppExe -PathType Leaf)) {
    if ($AppExe) { Bad "Файл не найден: $AppExe" }
    if ($Unattended) { Stop-Audit 'Не указан путь к rhizomind.exe (-AppExe).' }
    $AppExe = ConvertFrom-TypedPath (Ask 'Полный путь к rhizomind.exe (портативная сборка или установленная; пусто — выход)')
    if (-not $AppExe) { Stop-Audit 'Путь к rhizomind.exe не указан.' }
  }

  $isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)

  if (-not $NoProcmon) {
    $Procmon = ConvertFrom-TypedPath $Procmon
    while (-not $Procmon -or -not (Test-Path -LiteralPath $Procmon -PathType Leaf)) {
      if ($Procmon) { Bad "Файл не найден: $Procmon" }
      if ($Unattended) { Stop-Audit 'Не указан путь к Procmon64.exe (-Procmon) — или запустите с -NoProcmon.' }
      $Procmon = ConvertFrom-TypedPath (Ask 'Полный путь к Procmon64.exe (Sysinternals Process Monitor; пусто — без Process Monitor)')
      if (-not $Procmon) {
        if (Confirm 'Продолжить без Process Monitor (как с ключом -NoProcmon: шаги 3 и 10 пропускаются, остаётся опрос соединений)?') {
          $NoProcmon = $true
          Note "`n**Без Process Monitor (выбрано в шаге 1):** шаги 3 и 10 пропущены, сеть — только опросом соединений."
          break
        }
        Stop-Audit 'Process Monitor не указан. Запустите скрипт снова с -Procmon <путь> или с -NoProcmon.'
      }
    }
  }
  if (-not $NoProcmon) {
    if ($ProcmonConfig) {
      Say 'Ключ -ProcmonConfig больше не нужен и не используется: скрипт сам сбрасывает настройки Process Monitor на время аудита.'
      Note '- Ключ -ProcmonConfig указан, но не используется (задача 3.3в): настройки сбрасывает скрипт.'
    }
    if (-not $isAdmin) {
      Bad 'Process Monitor работает только с правами администратора (ставит свой драйвер).'
      Say '  Windows покажет запрос UAC несколько раз (пробная и основная запись на шаге 3, остановка и выгрузка на шаге 10) — подтверждайте.'
      Say '  Лучше перезапустить PowerShell «от имени администратора». Без прав — запустите скрипт с ключом -NoProcmon.'
      if (-not (Confirm 'Продолжить с запросами UAC?')) {
        Stop-Audit 'Перезапустите PowerShell от имени администратора или запустите скрипт с -NoProcmon.'
      }
    }
    $drive = Get-Item -LiteralPath $OutDir
    $free = $drive.PSDrive.Free
    if ($free -and $free -lt 20GB) {
      Bad ("Свободно на диске {0}: {1:N1} ГБ. Журнал Process Monitor за час — 5–20 ГБ, плюс столько же CSV при разборе." -f $drive.PSDrive.Name, ($free / 1GB))
      if (-not (Confirm 'Всё равно продолжить?' $true)) { Stop-Audit 'Освободите место или укажите -OutDir на другом диске.' }
    }
  }

  $os = Get-CimInstance Win32_OperatingSystem
  $wv = $null
  foreach ($k in @(
      'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
      'HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
      'HKCU:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}')) {
    if (-not $wv) { $wv = (Get-ItemProperty -LiteralPath $k -ErrorAction SilentlyContinue).pv }
  }
  $wlid = Get-Service -Name wlidsvc -ErrorAction SilentlyContinue
  $appHash = (Get-FileHash -LiteralPath $AppExe -Algorithm SHA256).Hash

  Note "- Windows: $($os.Caption) $($os.Version), сборка $($os.BuildNumber)"
  Note "- WebView2 Runtime: $wv"
  Note "- rhizomind.exe: SHA-256 $appHash"
  Note "- Запуск от администратора: $isAdmin"
  Note "- Process Monitor: $(if ($NoProcmon) { 'нет (-NoProcmon)' } else { 'да; настройки сброшены скриптом на время аудита (без фильтра, все классы событий, запись в файл)' })"
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
  if (-not $isAdmin) { Say 'Без прав администратора журнал DNS не включить, системная очередь WER может быть недоступна.' }
  Wait-Enter

  # --- Шаг 2. Снимок «до» ----------------------------------------------------

  Stage 'Снимок до запуска'
  $before = @{
    temp    = @(Get-ChildItem -LiteralPath $env:TEMP -Directory -Filter 'rhizomind-webview-*' -ErrorAction SilentlyContinue | ForEach-Object Name)
    profile = @(Get-ChildItem -LiteralPath (Join-Path $env:LOCALAPPDATA 'Rhizomind') -ErrorAction SilentlyContinue | ForEach-Object Name)
    tokens  = Get-TokenCacheSnapshot
    wer     = Get-WerQueue
  }
  Note "- Остатки временных папок движка в %TEMP%: $($before.temp.Count)"
  Note "- Файлы в %LOCALAPPDATA%\Rhizomind: $($before.profile -join ', ')"
  Note "- Файлов в TokenBroker\Cache: $($before.tokens.Count)"
  foreach ($s in $before.wer.Status) { Note "- Очередь WER: $s" }
  Good 'Снимок сделан.'

  if ($isAdmin -and (Confirm 'Включить журнал DNS-клиента Windows на время аудита (после аудита вернётся как было)?')) {
    $dnsLog = Get-WinEvent -ListLog $DnsLogName
    $script:DnsWasEnabled = [bool]$dnsLog.IsEnabled
    if (-not $script:DnsWasEnabled) {
      $dnsLog.IsEnabled = $true
      $dnsLog.SaveChanges()
      $script:DnsChanged = $true
    }
    $script:DnsUsed = $true
    Good 'Журнал DNS включён.'
    Note '- Журнал DNS-клиента: включён на время аудита.'
  } else {
    $script:DnsUsed = $false
    Note '- Журнал DNS-клиента: не включался.'
  }
  Wait-Enter

  # --- Шаг 3. Process Monitor ------------------------------------------------

  Stage 'Запуск Process Monitor'
  $captureStart = Get-Date
  if ($NoProcmon) {
    Note '- Пропущен: без Process Monitor (-NoProcmon).'
    Say 'Пропущен: без Process Monitor.'
  } else {
    Say 'Process Monitor будет записывать все события машины в файл capture.pml в папке результатов.'
    Say 'Окно Process Monitor может появиться свёрнутым — не закрывайте его до шага 10.'
    while ((Get-ProcmonProcess).Count -gt 0) {
      Bad 'Process Monitor уже запущен (возможно, со своим фильтром). Закройте все его окна — скрипт запустит его сам.'
      if ($Unattended) { Stop-Audit 'Process Monitor уже запущен.' }
      Wait-Enter 'Закройте Process Monitor и нажмите Enter'
    }
    # Сброс настроек (решение 107а): в первом аудите журнал вышел без событий
    # Rhizomind — вероятно, сохранённый на машине фильтр или выключенные классы.
    $backup = Join-Path $OutDir 'procmon-settings-backup.reg'
    Reset-ProcmonSetting $backup
    if ($script:ProcmonKeyExisted) {
      Note "- Настройки Process Monitor этой машины сохранены в ``procmon-settings-backup.reg`` и на время аудита сброшены (вернутся в конце)."
    } else {
      Note '- Настроек Process Monitor на машине не было; ключ, созданный на время аудита, в конце удаляется.'
    }
    if (-not $isAdmin) { Say 'Сейчас Windows спросит разрешение (UAC) для Process Monitor — подтверждайте каждый раз.' }

    Say 'Пробная запись (около 20 с): видит ли Process Monitor события тестового процесса — файлы, реестр, процессы.'
    $probe = Invoke-ProcmonProbe
    Note ("- Пробная запись: событий всего {0} ({1}); тестового процесса cmd.exe (PID {2}): {3}" -f $probe.Rows, (Format-ClassCounter $probe.AllClasses), ($probe.Pids -join ', '), (Format-ClassCounter $probe.Classes))
    if (-not $probe.Ok) {
      Note "- **Пробная запись не прошла:** $($probe.Reason)."
      Stop-Audit ("Пробная запись Process Monitor не прошла: $($probe.Reason). С таким журналом аудит ничего не докажет — он остановлен до запуска Rhizomind. " +
        'Что сделать: закройте все окна Process Monitor; запускайте PowerShell от администратора; возьмите свежий Process Monitor с сайта Microsoft Sysinternals; ' +
        'если стоит антивирус или EDR, он может мешать драйверу Process Monitor. Затем запустите скрипт снова. Подробно — инструкция, раздел «Если итог: не доказано».')
    }
    Good "Пробная запись: Process Monitor видит события тестового процесса ($(Format-ClassCounter $probe.Classes))."

    Start-ProcmonCapture $Pml
    $captureStart = Get-Date
    Note "- Запись Process Monitor начата: $($captureStart.ToString('HH:mm:ss')) (без фильтра, в файл capture.pml)"
    Good 'Запись идёт.'
  }
  Wait-Enter

  # --- Шаг 4. Запуск Rhizomind и флаги движка --------------------------------

  Stage 'Запуск Rhizomind и проверка флагов движка'
  # Единственный экземпляр: второй запуск передал бы команду уже открытому
  # окну и вышел — дерево процессов было бы чужим.
  while (@(Get-Process -Name 'rhizomind' -ErrorAction SilentlyContinue).Count -gt 0) {
    Bad 'Rhizomind уже запущен. Закройте все его окна (скрипт должен запустить его сам).'
    if ($Unattended) { Stop-Audit 'Rhizomind уже запущен.' }
    Wait-Enter 'Закройте Rhizomind и нажмите Enter'
  }
  Step 'Скрипт запустит Rhizomind. Окно может появиться через 1–3 секунды.'
  $appStart = Get-Date
  $proc = Start-Process -FilePath $AppExe -PassThru
  $script:RootPid = $proc.Id
  $browser = $null
  $tree = @()
  $deadline = (Get-Date).AddSeconds(20)
  while (-not $browser -and (Get-Date) -lt $deadline) {
    Start-Sleep -Seconds 2
    $tree = @(Get-AppTree)
    $browser = $tree | Where-Object { $_.Name -ieq 'msedgewebview2.exe' -and $_.CommandLine -notmatch '--type=' } | Select-Object -First 1
  }
  if (-not $browser -and -not $Unattended) {
    Bad 'Процесс браузера WebView2 не найден. Открылось ли окно Rhizomind? Подождите и нажмите Enter для повторной проверки.'
    Wait-Enter
    $tree = @(Get-AppTree)
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
  $script:FlagViolations = 0
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
      $mark = ''; if (-not $ok) { $mark = ' — **нарушение**'; $script:FlagViolations++ }
      Note ("| ``{0}`` | {1} | {2}{3} |" -f $e.flag, $want, $got, $mark)
      if ($ok) { Good "$($e.flag): $got" } else { Bad "$($e.flag): $got (ожидалось: $want)" }
    }
    Note ''
    Note 'Командная строка процесса браузера движка (пути профиля заменены условными обозначениями):'
    Note ''
    Note '```'
    Note (Protect-UserText $cmd)
    Note '```'
    $udd = [regex]::Match($cmd, '--user-data-dir="?([^"]+)"?').Groups[1].Value
    $script:EngineDir = Split-Path -Parent $udd
    $engineLong = Get-LongPath $script:EngineDir
    if (-not $engineLong) { $engineLong = $script:EngineDir }
    $inTemp = (Split-Path -Leaf $script:EngineDir) -like 'rhizomind-webview-*'
    $underTemp = $false
    foreach ($t in (Get-PathForms $env:TEMP)) { if ($engineLong.StartsWith($t + '\', [StringComparison]::OrdinalIgnoreCase) -or $script:EngineDir.StartsWith($t + '\', [StringComparison]::OrdinalIgnoreCase)) { $underTemp = $true } }
    Note ''
    Note "- Папка данных движка во временной папке ``%TEMP%\rhizomind-webview-*``: $($inTemp -and $underTemp)"
    if ($inTemp -and $underTemp) { Good "Папка данных: $engineLong" } else { Bad "Папка данных не во временной папке: $engineLong"; $script:FlagViolations++ }
  } else {
    Note '- **Процесс браузера WebView2 не найден** — флаги не проверены.'
    $script:FlagViolations++
  }
  # Через 60 с после запуска: запись Process Monitor продолжается (окно не
  # закрыто, драйвер не выгружен). Что он видит события именно этого запуска,
  # без остановки записи проверить нельзя — это проверила пробная запись на
  # шаге 3 с теми же настройками; события корневого процесса считает шаг 10.
  if (-not $NoProcmon) {
    $wait = [int][Math]::Ceiling(60 - ((Get-Date) - $appStart).TotalSeconds)
    if ($wait -gt 0) {
      Say "Через $wait с скрипт проверит, что Process Monitor продолжает запись."
      Start-Sleep -Seconds $wait
    }
    $pmAlive = (Get-ProcmonProcess).Count -gt 0
    $pmlMb = 0
    if (Test-Path -LiteralPath $Pml) { $pmlMb = [Math]::Round((Get-Item -LiteralPath $Pml).Length / 1MB, 1) }
    Note "- Через 60 с после запуска Rhizomind: Process Monitor работает — $pmAlive; capture.pml — $pmlMb МБ"
    if (-not $pmAlive) {
      Stop-Audit 'Process Monitor перестал работать (окно закрыто или процесс завершился) — журнал этого запуска не будет полным. Закройте Rhizomind и запустите скрипт снова; окно Process Monitor не закрывайте до шага 10.'
    }
    Good "Process Monitor пишет журнал (capture.pml — $pmlMb МБ)."
  }
  Wait-Enter

  # --- Шаг 5. Простой --------------------------------------------------------

  Stage 'Простой: фоновые обращения движка'
  Say 'Ничего не делайте в Rhizomind: окно открыто, программа простаивает.'
  Say 'Здесь проявились бы фоновые службы движка: конфигурация и эксперименты, обновление компонентов, отчёты.'
  if ($Unattended) {
    $idleSec = [Math]::Max(5, $IdleSeconds)
  } else {
    $idleMin = 0
    while ($true) {
      $answer = Ask 'Сколько минут ждать' '10'
      if ([int]::TryParse(([string]$answer).Trim(), [ref]$idleMin) -and $idleMin -gt 0 -and $idleMin -le 240) { break }
      Say '  Введите целое число минут от 1 до 240.'
    }
    $idleSec = $idleMin * 60
  }
  Note "- Простой: $idleSec с."
  Watch-Network 'простой' $idleSec

  # --- Шаги 6–8. Сценарии ----------------------------------------------------

  Stage 'Сценарий: хранилище и заметки'
  $vaultPath = ''
  if ($Unattended) {
    Note '- Пропущен: автоматический режим.'
  } else {
    Step 'Если открылось «Управление хранилищами» — создайте хранилище «Аудит» в новой папке на диске C: (кнопка «Обзор…»).'
    Step 'Создайте 3 заметки и папку; в заметке наберите несколько абзацев, список, задачу - [ ], код в `обратных кавычках`.'
    Step 'Вставьте вики-ссылку [[Вторая заметка]] и перейдите по ней; создайте заметку по неразрешённой ссылке.'
    Step 'Ctrl+O — найдите заметку; Alt+← и Alt+→ — назад и вперёд; Ctrl+E — режим исходного текста и обратно.'
    Step 'Переименуйте заметку в дереве (F2), перетащите в папку, удалите заметку (в корзину).'
    Step 'Откройте папку хранилища в Проводнике, измените заметку Блокнотом и сохраните — вернитесь в Rhizomind.'
    Watch-Network 'хранилище и заметки'
    while ($true) {
      $vaultPath = ConvertFrom-TypedPath (Ask 'Полный путь к папке хранилища (для разбора журнала; Enter — взять из vaults.json)')
      $vaultPath = $vaultPath.TrimEnd('\')
      if (-not $vaultPath -or (Test-Path -LiteralPath $vaultPath -PathType Container)) { break }
      Bad "Папка не найдена: $vaultPath"
    }
  }

  Stage 'Сценарий: поля ввода, контекстное меню, настройки'
  if ($Unattended) {
    Note '- Пропущен: автоматический режим.'
  } else {
    Step 'Наберите слова с ошибками («превет», «малако») в заметке, в поиске Ctrl+O, в поле переименования, в поиске горячих клавиш и фильтре папок в настройках.'
    Step 'Проверка орфографии выключена во всех полях (задача 3.3а): красных подчёркиваний быть не должно, словари не скачиваются.'
    Step 'Откройте «Настройки хранилища»: смените тему, ширину текста, размер шрифта; назначьте горячую клавишу; закройте настройки.'
    Step 'Щёлкните правой кнопкой: в тексте заметки, на выделенном слове, на пустом месте окна. НИЧЕГО в меню не выбирайте — только перепишите пункты.'
    Watch-Network 'поля, меню, настройки'
    $underline = Confirm 'Были ли красные подчёркивания слов с ошибками хоть в одном поле?'
    Note "- Подчёркивания проверки орфографии: $(if ($underline) { '**были** — нарушение решения задачи 3.3а' } else { 'не было' })"
    $dict = @()
    if ($script:EngineDir) {
      $dict = @(Get-ChildItem -LiteralPath $script:EngineDir -Recurse -Force -ErrorAction SilentlyContinue | Where-Object { $_.Name -like '*.bdic' -or ($_.PSIsContainer -and $_.Name -eq 'Dictionaries') })
    }
    if ($dict.Count -eq 0) { Good 'Словарей Hunspell (*.bdic) во временной папке движка нет.' } else { Bad "Во временной папке движка словари: $(@($dict | ForEach-Object Name) -join ', ')" }
    Note "- Словари Hunspell (*.bdic, Dictionaries) во временной папке движка: $(if ($dict.Count) { '**есть** — ' + (@($dict | ForEach-Object Name) -join ', ') } else { 'нет' })"
    $menu = Ask 'Перечислите пункты контекстного меню в тексте заметки и на выделении (через запятую)'
    Note "- Пункты контекстного меню: $menu"
  }

  Stage 'Сценарий: внешние ссылки'
  if ($Unattended) {
    Note '- Пропущен: автоматический режим.'
  } else {
    Step 'В заметке наберите [Пример](https://example.com) и щёлкните по ссылке. В окне подтверждения нажмите «Отмена».'
    Step 'Щёлкните ещё раз и нажмите «Открыть в браузере»: откроется системный браузер. Его соединения — норма, это действие пользователя; закройте браузер.'
    Step 'Наберите [Почта](mailto:test@example.com), щёлкните — «Отмена».'
    Step 'Наберите [Файл](file:///C:/Windows/win.ini) и щёлкните — ничего не должно открыться.'
    Watch-Network 'внешние ссылки'
    $cancelOk = Confirm 'После «Отмена» браузер не открывался?'
    $fileOk = Confirm 'Ссылка file: ничего не открыла?'
    Note "- «Отмена» не открыла браузер: $cancelOk; ссылка file: ничего не открыла: $fileOk"
  }

  # --- Шаг 9. Второе хранилище и выход ---------------------------------------

  Stage 'Второе хранилище и выход'
  if ($Unattended) {
    Say 'Автоматический режим: скрипт закрывает окно Rhizomind (как крестиком «×»).'
    $rootProc = Get-Process -Id $script:RootPid -ErrorAction SilentlyContinue
    $closed = $false
    if ($rootProc) { $closed = $rootProc.CloseMainWindow() }
    Note "- Окно закрыто сообщением WM_CLOSE (как «×»): $closed"
  } else {
    Step 'Через переключатель слева внизу откройте «Управление хранилищами», создайте второе хранилище, откройте его.'
    Step 'Наберите текст в заметке второго хранилища и сразу закройте окна крестиком «×» (все окна Rhizomind).'
    Watch-Network 'второе хранилище и выход'
  }
  $waitUntil = (Get-Date).AddSeconds(30)
  while (@(Get-AppTree | Where-Object { Test-OwnImage $_.Name }).Count -gt 0 -and (Get-Date) -lt $waitUntil) { Start-Sleep -Seconds 1 }
  Start-Sleep -Seconds 3
  $left = @(Get-AppTree | Where-Object { Test-OwnImage $_.Name })
  $script:ExitProblems = 0
  if ($left.Count -gt 0) { Bad "Процессы Rhizomind живы через 30 с после закрытия: $($left.Count)"; $script:ExitProblems++ } else { Good 'Все процессы Rhizomind завершились.' }
  Note "- Процессы Rhizomind после выхода: $($left.Count)"
  $tempAfter = @(Get-ChildItem -LiteralPath $env:TEMP -Directory -Filter 'rhizomind-webview-*' -ErrorAction SilentlyContinue | ForEach-Object Name)
  $newTemp = @($tempAfter | Where-Object { $before.temp -notcontains $_ })
  if ($newTemp.Count -eq 0) { Good 'Временная папка движка удалена.' } else { Bad "Осталась временная папка: $($newTemp -join ', ')"; $script:ExitProblems++ }
  Note "- Новые временные папки движка после выхода: $($newTemp.Count)"

  $tokenChanges = Compare-TokenCache $before.tokens (Get-TokenCacheSnapshot)
  if ($tokenChanges.Count -eq 0) {
    Note '- TokenBroker\Cache: без изменений.'
  } else {
    Note '- TokenBroker\Cache — изменения:'
    foreach ($c in $tokenChanges) {
      Note "  - ``$($c.Name)``: $($c.Change) — $($c.Verdict)"
      if ($c.Violation) { Bad "TokenBroker\Cache: $($c.Name) $($c.Change)"; $script:ExitProblems++ }
    }
  }
  $werAfter = Get-WerQueue
  foreach ($s in $werAfter.Status) { Note "- Очередь WER после выхода: $s" }
  $newWer = @($werAfter.Names | Where-Object { $before.wer.Names -notcontains $_ })
  Note "- Новые отчёты WER в очереди: $($newWer.Count) $(if ($newWer.Count) { '(' + ($newWer -join '; ') + ')' })"

  $vaultsJson = Join-Path $env:LOCALAPPDATA 'Rhizomind\vaults.json'
  $vaultRoots = New-Object System.Collections.ArrayList
  if ($vaultPath) { [void]$vaultRoots.Add($vaultPath) }
  if (Test-Path -LiteralPath $vaultsJson) {
    $list = Get-Content -Raw -Encoding UTF8 -LiteralPath $vaultsJson | ConvertFrom-Json
    $fields = @($list.PSObject.Properties.Name)
    Note "- vaults.json: поля верхнего уровня $($fields -join ', '); хранилищ $(@($list.vaults).Count)"
    foreach ($v in @($list.vaults)) {
      if ($v.path) { [void]$vaultRoots.Add([string]$v.path) }
      Note ("  - хранилище: поля {0}" -f (@($v.PSObject.Properties.Name) -join ', '))
    }
    if ($Unattended) {
      Note '- В vaults.json нет имён и текста заметок: не проверялось глазами (автоматический режим).'
    } else {
      Say ''
      Say 'Содержимое vaults.json:'
      Get-Content -Encoding UTF8 -LiteralPath $vaultsJson | Write-Host
      $noNotes = Confirm 'В vaults.json нет имён заметок и текста (только имена и пути хранилищ, размеры окна)?'
      Note "- В vaults.json нет имён и текста заметок (проверено глазами): $noNotes"
    }
  } else {
    Note '- vaults.json: нет.'
  }
  Wait-Enter

  # --- Шаг 10. Остановить запись и разобрать журнал --------------------------

  Stage 'Остановка Process Monitor и разбор журнала'
  $ownPids = New-Object System.Collections.Generic.List[int]
  $ownPids.Add([int]$script:RootPid)
  foreach ($k in $script:Seen.Keys) { if (Test-OwnImage $script:Seen[$k].Name) { $ownPids.Add([int]$k) } }
  $analysis = $null
  if ($NoProcmon) {
    Note '- Пропущен: без Process Monitor (-NoProcmon).'
    Say 'Пропущен: без Process Monitor.'
  } else {
    Say 'Запись останавливается, журнал выгружается в CSV и разбирается.'
    Say 'Выгрузка часа записи — 10–40 минут, CSV размером с журнал (гигабайты). Окно не закрывайте.'
    if (-not $isAdmin) { Say 'Windows спросит разрешение (UAC) для остановки Process Monitor — подтвердите.' }
    Stop-Procmon
    Start-Sleep -Seconds 3
    # Те же гарантии, что при записи: настройки ещё сброшены, /NoFilter,
    # /SaveAs без /SaveApplyFilter — в CSV все события журнала.
    Convert-ProcmonLog $Pml $Csv
    while (-not (Test-Path -LiteralPath $Csv)) {
      Bad 'CSV не получился. Откройте capture.pml в Process Monitor вручную: Filter → Reset Filter, затем File → Save… → «All events», формат CSV, в эту папку как capture.csv.'
      if (-not (Confirm 'CSV сохранён вручную (иначе — без разбора журнала)?')) { break }
    }
    if (Test-Path -LiteralPath $Csv) {
      $analysis = Invoke-ProcmonAnalysis -CsvPath $Csv -OwnPids $ownPids.ToArray() -RootPid $script:RootPid -VaultRoots $vaultRoots.ToArray() -EngineDir $script:EngineDir
      Write-ProcmonReport $analysis
    } else {
      Note '- **Журнал Process Monitor не разобран** (CSV нет).'
    }
  }

  if ($script:LiveFindings.Count -gt 0) {
    Note "`n### Соединения, замеченные опросом во время работы`n"
    Note '| Этап | Вид | Процесс | Номер | Адрес | Состояние |'
    Note '|---|---|---|---|---|---|'
    foreach ($f in $script:LiveFindings) { Note "| $($f.Phase) | $($f.Kind) | $($f.Process) | $($f.Pid) | $($f.Remote) | $($f.State) |" }
  }

  if ($script:DnsUsed) {
    $ownSet = New-Object 'System.Collections.Generic.HashSet[int]'
    foreach ($p in $ownPids) { [void]$ownSet.Add($p) }
    if ($analysis) { foreach ($p in $analysis.Own) { [void]$ownSet.Add($p) } }
    $dnsEvents = @(Get-WinEvent -FilterHashtable @{ LogName = $DnsLogName; StartTime = $captureStart } -ErrorAction SilentlyContinue)
    $dnsOwn = @($dnsEvents | Where-Object { $ownSet.Contains([int]$_.ProcessId) })
    Note "`n### Запросы DNS процессов Rhizomind и движка (журнал DNS-клиента)`n"
    $names = @(Get-DnsQueryName $dnsOwn)
    if ($names.Count -eq 0) { Note 'Нет.' } else { foreach ($n in $names) { Note "- ``$n``" } }
    Note "`nЖурнал DNS за время аудита — всего событий $($dnsEvents.Count), имён у всех процессов: $(@(Get-DnsQueryName $dnsEvents).Count) (контроль: журнал видит запросы, например браузера на шаге 8)."
  }
  Good 'Разбор закончен.'
  Wait-Enter

  # --- Шаг 11. Итог ----------------------------------------------------------

  Stage 'Итог'
  Write-NetVerdict -Analysis $analysis -LiveCount $script:LiveFindings.Count -NoPm ([bool]$NoProcmon) -WithPoll $true
  Note "- Флаги и папка движка: $(if ($script:FlagViolations) { "**нарушений $($script:FlagViolations)**" } else { 'как ожидается' }); выход: $(if ($script:ExitProblems) { "**проблем $($script:ExitProblems)**" } else { 'чисто' })"
  Say ''
  Say "Папка результатов: $OutDir"
  Say 'Пришлите results.md и summary-*.csv. Файлы capture.pml и capture.csv не пересылайте:'
  Say 'в них все события машины (имена файлов, пользователя). Удалите их сами, когда разбор закончен.'
  Note "`nПримечания владельца:`n"
  $extra = Ask 'Что ещё заметили (Enter — ничего)'
  if ($extra) { Note $extra }
  Note "`nАудит завершён: $(Get-Date -Format 'yyyy-MM-dd HH:mm')."
  $script:Completed = $true
} catch [System.OperationCanceledException] {
  Bad $_.Exception.Message
  if ($script:Results -and (Test-Path -LiteralPath $script:Results)) { Note "`n**Аудит прерван:** $($_.Exception.Message)" }
  $script:ExitCode = 2
} catch {
  Bad "Сбой скрипта: $($_.Exception.Message)"
  Bad "  строка $($_.InvocationInfo.ScriptLineNumber): $([string]$_.InvocationInfo.Line)"
  if ($script:Results -and (Test-Path -LiteralPath $script:Results)) { Note "`n**Сбой скрипта:** $($_.Exception.Message) (строка $($_.InvocationInfo.ScriptLineNumber))" }
  $script:ExitCode = 1
} finally {
  if ($script:ProcmonStarted -and -not $script:ProcmonStopped) {
    try { Stop-Procmon; Write-Host '  Process Monitor остановлен.' -ForegroundColor Yellow } catch { Write-Host "  !!  Не удалось остановить Process Monitor: $($_.Exception.Message). Закройте его окно сами." -ForegroundColor Red }
  }
  if ($script:ProcmonSettingsReset) {
    try {
      Restore-ProcmonSetting
      if ($script:ProcmonKeyExisted) { Write-Host '  Настройки Process Monitor возвращены из procmon-settings-backup.reg.' -ForegroundColor Yellow }
    } catch { Write-Host "  !!  Не удалось вернуть настройки Process Monitor: $($_.Exception.Message). Верните сами: reg import `"$($script:ProcmonBackup)`"" -ForegroundColor Red }
  }
  if ($script:DnsChanged) {
    try {
      $dnsLog = Get-WinEvent -ListLog $DnsLogName
      $dnsLog.IsEnabled = $script:DnsWasEnabled
      $dnsLog.SaveChanges()
      $script:DnsChanged = $false
      Write-Host '  Журнал DNS возвращён в прежнее состояние (выключен).' -ForegroundColor Yellow
    } catch { Write-Host "  !!  Не удалось вернуть журнал DNS: $($_.Exception.Message). Выключите: wevtutil sl $DnsLogName /e:false" -ForegroundColor Red }
  }
  # Сбой, остановка или Ctrl+C (после Ctrl+C catch не выполняется — только finally).
  if ($script:RootPid -and -not $script:Completed) {
    $alive = @(Get-AppTree | Where-Object { Test-OwnImage $_.Name })
    if ($alive.Count -gt 0) { Write-Host '  Rhizomind ещё запущен — закройте его окно сами.' -ForegroundColor Yellow }
  }
}
exit $script:ExitCode

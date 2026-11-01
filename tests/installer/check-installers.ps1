# Проверка установщиков на машине CI (задача 3.4, критерий С9.9 — предварительно,
# до ручной проверки владельцем на второй машине): MSI ставится для всех
# пользователей (нужны права администратора — у раннера они есть), NSIS — для
# текущего пользователя; программа из обоих запускается; удаление убирает всё,
# что поставил установщик. Что именно пишет каждый установщик, записывается в
# отчёт (Markdown) — доказательство для docs/SECURITY.md «Записи вне хранилища».
#
# Запуск (Windows PowerShell 5.1, от администратора, на чистой машине):
#   powershell -NoProfile -ExecutionPolicy Bypass -File tests/installer/check-installers.ps1 `
#     -Msi <файл .msi> -Nsis <файл -setup.exe> -Report <отчёт .md>
# Код выхода: 0 — всё совпало с ожидаемым, 1 — есть расхождения (FAIL в выводе).
param(
  [Parameter(Mandatory = $true)] [string] $Msi,
  [Parameter(Mandatory = $true)] [string] $Nsis,
  [Parameter(Mandatory = $true)] [string] $Report
)
$ErrorActionPreference = 'Stop'
try { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8 } catch { $null = $_ }

$script:Failures = 0
$script:Lines = New-Object System.Collections.Generic.List[string]
function Note([string] $text) { $script:Lines.Add($text); Write-Host $text }
function Check([string] $name, [bool] $condition, [string] $detail = '') {
  if ($condition) { Note "- OK: $name" } else { Note "- **FAIL**: $name $detail"; $script:Failures++ }
}

$product = 'Rhizomind'
$manufacturer = 'Rhizomind contributors'
$programFilesDir = Join-Path $env:ProgramFiles $product
$userProgramsDir = Join-Path $env:LOCALAPPDATA "Programs\$product"
$dataDir = Join-Path $env:LOCALAPPDATA $product
$userStartMenu = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'
$commonStartMenu = Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs'
$userDesktop = [Environment]::GetFolderPath('Desktop')
$publicDesktop = Join-Path $env:PUBLIC 'Desktop'

# Места, куда установщики Rhizomind пишут или могли бы писать. Папка данных
# программы (%LOCALAPPDATA%\Rhizomind, vaults.json) — не установщик: её пишет
# сама программа, удаление по умолчанию её не трогает.
$watchedPaths = @(
  $programFilesDir,
  $userProgramsDir,
  (Join-Path $userStartMenu $product), (Join-Path $userStartMenu "$product.lnk"),
  (Join-Path $commonStartMenu $product), (Join-Path $commonStartMenu "$product.lnk"),
  (Join-Path $userDesktop "$product.lnk"), (Join-Path $publicDesktop "$product.lnk")
)
$watchedKeys = @(
  "HKCU:\Software\$manufacturer",
  "HKLM:\SOFTWARE\$manufacturer",
  "HKLM:\SOFTWARE\WOW6432Node\$manufacturer",
  "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\$product",
  "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$product"
)
$uninstallRoots = @(
  'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall',
  'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall',
  'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall'
)
# Верхний уровень мест, где появление нового элемента — повод разобраться.
$topLevel = @(
  @{ Name = '%LOCALAPPDATA%'; Path = $env:LOCALAPPDATA },
  @{ Name = '%LOCALAPPDATA%\Programs'; Path = (Join-Path $env:LOCALAPPDATA 'Programs') },
  @{ Name = '%APPDATA% (Roaming)'; Path = $env:APPDATA },
  @{ Name = '%ProgramData%'; Path = $env:ProgramData },
  @{ Name = '%ProgramFiles%'; Path = $env:ProgramFiles },
  @{ Name = 'HKCU\Software'; Path = 'HKCU:\Software' },
  @{ Name = 'HKLM\SOFTWARE'; Path = 'HKLM:\SOFTWARE' },
  @{ Name = 'HKCU\Software\Classes'; Path = 'HKCU:\Software\Classes' },
  @{ Name = 'HKCU\...\Run'; Path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'; Values = $true }
)

function Get-UninstallEntries {
  $found = @()
  foreach ($root in $uninstallRoots) {
    if (-not (Test-Path -LiteralPath $root)) { continue }
    foreach ($key in Get-ChildItem -LiteralPath $root -ErrorAction SilentlyContinue) {
      $props = Get-ItemProperty -LiteralPath $key.PSPath -ErrorAction SilentlyContinue
      if ($props -and $props.DisplayName -eq $product) { $found += $key.Name }
    }
  }
  return $found
}

# Всё, что есть сейчас из наблюдаемых мест: строки «что — где».
function Get-Traces {
  $traces = @()
  foreach ($p in $watchedPaths) {
    if (Test-Path -LiteralPath $p) {
      $traces += "файл: $p"
      if ((Get-Item -LiteralPath $p).PSIsContainer) {
        foreach ($f in Get-ChildItem -LiteralPath $p -Recurse -Force -ErrorAction SilentlyContinue) {
          $traces += "файл: $($f.FullName)"
        }
      }
    }
  }
  foreach ($k in $watchedKeys) {
    if (Test-Path -LiteralPath $k) {
      $traces += "реестр: $k"
      foreach ($sub in @(Get-Item -LiteralPath $k) + @(Get-ChildItem -LiteralPath $k -Recurse -ErrorAction SilentlyContinue)) {
        foreach ($v in $sub.GetValueNames()) {
          $name = if ($v -eq '') { '(по умолчанию)' } else { $v }
          $traces += "реестр: $($sub.Name) : $name = $($sub.GetValue($v))"
        }
      }
    }
  }
  foreach ($u in Get-UninstallEntries) { $traces += "реестр (удаление программ): $u" }
  $traces += "PATH пользователя: $([Environment]::GetEnvironmentVariable('Path', 'User'))"
  $traces += "PATH системы: $([Environment]::GetEnvironmentVariable('Path', 'Machine'))"
  return $traces
}

function Get-TopLevel {
  $result = @{}
  foreach ($t in $topLevel) {
    if (-not (Test-Path -LiteralPath $t.Path)) { $result[$t.Name] = @(); continue }
    if ($t.Values) {
      $result[$t.Name] = @((Get-Item -LiteralPath $t.Path).GetValueNames())
    } else {
      $result[$t.Name] = @(Get-ChildItem -LiteralPath $t.Path -Force -ErrorAction SilentlyContinue | ForEach-Object { $_.PSChildName })
    }
  }
  return $result
}

function Compare-TopLevel($before, $after, [string] $stage) {
  $new = @()
  foreach ($t in $topLevel) {
    foreach ($n in $after[$t.Name]) { if ($before[$t.Name] -notcontains $n) { $new += "$($t.Name)\$n" } }
  }
  if ($new.Count -eq 0) { Note "- Новых элементов верхнего уровня ($stage): нет" }
  else { Note "- Новые элементы верхнего уровня ($stage): $($new -join '; ')" }
  return $new
}

function Show-List([string] $title, $items) {
  Note ''
  Note "$title"
  Note ''
  Note '```'
  foreach ($i in $items) { Note $i }
  Note '```'
}

# Запуск установленной программы: процесс жив через 10 с — окно открылось.
function Test-Launch([string] $exe) {
  $proc = Start-Process -FilePath $exe -PassThru
  Start-Sleep -Seconds 10
  $alive = -not $proc.HasExited
  Check "программа запускается ($exe) и работает 10 с" $alive "exit=$(if ($proc.HasExited) { $proc.ExitCode })"
  if ($alive) {
    [void]$proc.CloseMainWindow()
    if (-not $proc.WaitForExit(20000)) { Stop-Process -Id $proc.Id -Force; $proc.WaitForExit() }
  }
  # Процессы WebView2 программы завершаются вместе с ней; ждём, чтобы удаление
  # не наткнулось на занятые файлы.
  for ($i = 0; $i -lt 30; $i++) {
    if (-not (Get-Process -Name 'rhizomind' -ErrorAction SilentlyContinue)) { break }
    Start-Sleep -Seconds 1
  }
}

function Wait-Until([scriptblock] $condition, [int] $seconds) {
  for ($i = 0; $i -lt $seconds; $i++) {
    if (& $condition) { return $true }
    Start-Sleep -Seconds 1
  }
  return (& $condition)
}

$baseline = Get-Traces
$baselineTop = Get-TopLevel
$baselinePath = @($baseline | Where-Object { $_ -like 'PATH*' })

Note '# Проверка установщиков Rhizomind'
Note ''
Note "Машина: $env:COMPUTERNAME, Windows $([Environment]::OSVersion.Version), PowerShell $($PSVersionTable.PSVersion); дата $(Get-Date -Format 'yyyy-MM-dd HH:mm')."
Note "MSI: $(Split-Path -Leaf $Msi); NSIS: $(Split-Path -Leaf $Nsis)."
Note ''
Check 'до установки следов установщиков нет' ((@($baseline | Where-Object { $_ -notlike 'PATH*' })).Count -eq 0) ($baseline -join '; ')
Check 'до установки нет папки данных программы' (-not (Test-Path -LiteralPath $dataDir))

# --- MSI ----------------------------------------------------------------------

Note ''
Note '## MSI (для всех пользователей, с правами администратора)'
Note ''
$log = Join-Path $env:TEMP 'rhizomind-msi-install.log'
$p = Start-Process -FilePath 'msiexec.exe' -ArgumentList @('/i', "`"$Msi`"", '/qn', '/norestart', '/l*v', "`"$log`"") -Wait -PassThru
Check 'msiexec /i /qn — код 0' ($p.ExitCode -eq 0) "code=$($p.ExitCode); журнал $log"
$msiInstalled = Get-Traces
$msiTop = Get-TopLevel
Check "программа в $programFilesDir" (Test-Path -LiteralPath (Join-Path $programFilesDir 'rhizomind.exe'))
Check 'ярлык в общем меню «Пуск»' (Test-Path -LiteralPath (Join-Path $commonStartMenu "$product\$product.lnk"))
Check 'запись в «Программы и компоненты» (HKLM)' ((@(Get-UninstallEntries | Where-Object { $_ -like 'HKEY_LOCAL_MACHINE*' })).Count -eq 1)
Check 'PATH не изменился' ($null -eq (Compare-Object $baselinePath @($msiInstalled | Where-Object { $_ -like 'PATH*' })))
Show-List 'Что поставил MSI (наблюдаемые места):' @($msiInstalled | Where-Object { $_ -notlike 'PATH*' })
[void](Compare-TopLevel $baselineTop $msiTop 'после установки MSI')
$installedFiles = @(Get-ChildItem -LiteralPath $programFilesDir -Recurse -File | ForEach-Object { "$($_.FullName) ($($_.Length) байт)" })
Show-List 'Файлы программы MSI:' $installedFiles

Test-Launch (Join-Path $programFilesDir 'rhizomind.exe')

$p = Start-Process -FilePath 'msiexec.exe' -ArgumentList @('/x', "`"$Msi`"", '/qn', '/norestart', '/l*v', "`"$(Join-Path $env:TEMP 'rhizomind-msi-uninstall.log')`"") -Wait -PassThru
Check 'msiexec /x /qn — код 0' ($p.ExitCode -eq 0) "code=$($p.ExitCode)"
$afterMsi = Get-Traces
$leftMsi = @($afterMsi | Where-Object { $baseline -notcontains $_ })
Check 'после удаления MSI следов установщика нет' ($leftMsi.Count -eq 0) ($leftMsi -join '; ')
[void](Compare-TopLevel $baselineTop (Get-TopLevel) 'после удаления MSI')
Note "- Папка данных программы $dataDir после удаления: $(if (Test-Path -LiteralPath $dataDir) { 'есть (не трогается — данные пользователя): ' + ((Get-ChildItem -LiteralPath $dataDir -Force | ForEach-Object { $_.Name }) -join ', ') } else { 'нет (программа не записала список хранилищ)' })"

# --- NSIS ---------------------------------------------------------------------

Note ''
Note '## NSIS (для текущего пользователя, без прав администратора)'
Note ''
$p = Start-Process -FilePath $Nsis -ArgumentList @('/S') -Wait -PassThru
Check 'установщик NSIS /S — код 0' ($p.ExitCode -eq 0) "code=$($p.ExitCode)"
$nsisInstalled = Get-Traces
$nsisTop = Get-TopLevel
Check "программа в $userProgramsDir" (Test-Path -LiteralPath (Join-Path $userProgramsDir 'rhizomind.exe'))
Check 'в папке данных программы нет файлов программы' (-not (Test-Path -LiteralPath (Join-Path $dataDir 'rhizomind.exe')))
Check 'ничего в Program Files' (-not (Test-Path -LiteralPath $programFilesDir))
Check 'запись в «Программы и компоненты» только в HKCU' (((@(Get-UninstallEntries)).Count -eq 1) -and ((@(Get-UninstallEntries | Where-Object { $_ -like 'HKEY_CURRENT_USER*' })).Count -eq 1))
Check 'ярлык в меню «Пуск» пользователя' (Test-Path -LiteralPath (Join-Path $userStartMenu "$product.lnk"))
Check 'PATH не изменился' ($null -eq (Compare-Object $baselinePath @($nsisInstalled | Where-Object { $_ -like 'PATH*' })))
Show-List 'Что поставил NSIS (наблюдаемые места):' @($nsisInstalled | Where-Object { $_ -notlike 'PATH*' })
[void](Compare-TopLevel $baselineTop $nsisTop 'после установки NSIS')

Test-Launch (Join-Path $userProgramsDir 'rhizomind.exe')

# Деинсталлятор NSIS копирует себя во временную папку и сразу возвращает
# управление — ждём, пока исчезнут запись об удалении и папка программы.
$uninstaller = Join-Path $userProgramsDir 'uninstall.exe'
$p = Start-Process -FilePath $uninstaller -ArgumentList @('/S') -Wait -PassThru
Check 'деинсталлятор NSIS /S — код 0' ($p.ExitCode -eq 0) "code=$($p.ExitCode)"
$gone = Wait-Until { (-not (Test-Path -LiteralPath $userProgramsDir)) -and ((@(Get-UninstallEntries)).Count -eq 0) } 120
Check 'папка программы и запись об удалении исчезли (до 120 с)' $gone
$afterNsis = Get-Traces
$leftNsis = @($afterNsis | Where-Object { $baseline -notcontains $_ })
Check 'после удаления NSIS следов установщика нет' ($leftNsis.Count -eq 0) ($leftNsis -join '; ')
[void](Compare-TopLevel $baselineTop (Get-TopLevel) 'после удаления NSIS')
Note "- Папка данных программы $dataDir после удаления: $(if (Test-Path -LiteralPath $dataDir) { 'есть (по умолчанию не трогается): ' + ((Get-ChildItem -LiteralPath $dataDir -Force | ForEach-Object { $_.Name }) -join ', ') } else { 'нет' })"

Note ''
Note "Итог: $(if ($script:Failures -eq 0) { 'расхождений нет' } else { "расхождений: $($script:Failures)" })."
$dir = Split-Path -Parent $Report
if ($dir -and -not (Test-Path -LiteralPath $dir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
[System.IO.File]::WriteAllLines($Report, $script:Lines, (New-Object System.Text.UTF8Encoding $false))
if ($script:Failures -gt 0) { exit 1 }
exit 0

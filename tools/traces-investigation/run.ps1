# Сценарии расследования следов под Process Monitor (задача 0.6).
# Запускается из .github/workflows/traces-investigation.yml на windows-latest.
#
# Каждая фаза: снимок «до» (файлы %LOCALAPPDATA%/%APPDATA%, экспорт HKCU,
# диспетчер учётных данных) → метка начала → сценарий → метка конца →
# снимок «после». Process Monitor пишет всё время; analyze.py раскладывает
# события по фазам по меткам времени.
param(
  [Parameter(Mandatory)] [string] $Job,
  [Parameter(Mandatory)] [string] $Probe,
  [Parameter(Mandatory)] [string] $Procmon,
  [string] $AppExe = '',
  [string] $WryProbe = ''
)
$ErrorActionPreference = 'Stop'
$out = Join-Path $env:RUNNER_TEMP 't06'
New-Item -ItemType Directory -Force -Path $out | Out-Null
$markers = [System.Collections.Generic.List[object]]::new()
$results = Join-Path $out 'results.jsonl'

function Mark([string] $phase, [string] $edge) {
  $now = [DateTime]::UtcNow
  $markers.Add([ordered]@{ phase = $phase; edge = $edge; filetime = $now.ToFileTimeUtc(); iso = $now.ToString('o') })
}

function Invoke-Probe([string[]] $arguments, [string] $phase) {
  $stdout = Join-Path $out ("probe-" + [guid]::NewGuid().ToString('N') + '.json')
  $p = Start-Process -FilePath $Probe -ArgumentList $arguments -RedirectStandardOutput $stdout -PassThru -Wait -WindowStyle Normal
  $text = (Get-Content -Raw -LiteralPath $stdout -ErrorAction SilentlyContinue)
  Remove-Item -LiteralPath $stdout -Force -ErrorAction SilentlyContinue
  if ($phase) {
    $obj = if ($text) { $text | ConvertFrom-Json -AsHashtable } else { @{ error = "нет вывода, код $($p.ExitCode)" } }
    $obj['phase'] = $phase
    Add-Content -LiteralPath $results -Value ($obj | ConvertTo-Json -Compress -Depth 6) -Encoding utf8
  }
  return $text
}

function Snap([string] $label) {
  $dir = Join-Path $out "snap\$label"
  New-Item -ItemType Directory -Force -Path $dir | Out-Null
  $rows = foreach ($root in @($env:LOCALAPPDATA, $env:APPDATA)) {
    Get-ChildItem -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue | ForEach-Object {
      $size = if ($_.PSIsContainer) { 'dir' } else { $_.Length }
      "{0}`t{1}`t{2}" -f $_.FullName, $size, $_.LastWriteTimeUtc.Ticks
    }
  }
  [IO.File]::WriteAllLines((Join-Path $dir 'files.tsv'), [string[]] $rows)
  & reg.exe export HKCU (Join-Path $dir 'hkcu.reg') /y | Out-Null
  $creds = Invoke-Probe @('creds') $null
  Set-Content -LiteralPath (Join-Path $dir 'creds.json') -Value $creds -Encoding utf8
}

function Get-Children([int] $parentId) {
  Get-CimInstance Win32_Process -Filter "ParentProcessId = $parentId" -ErrorAction SilentlyContinue
}

# --- сценарии ------------------------------------------------------------------

function Scenario-Idle([string] $phase) {
  Start-Sleep -Seconds 25
  Add-Content -LiteralPath $results -Value (@{ phase = $phase; idle = 25 } | ConvertTo-Json -Compress) -Encoding utf8
}

function Scenario-App([string] $phase) {
  $p = Start-Process -FilePath $AppExe -PassThru
  $null = $p.Handle # без этого ExitCode после выхода пуст
  $browser = $null
  for ($i = 0; $i -lt 60 -and -not $browser; $i++) {
    Start-Sleep -Milliseconds 500
    $browser = Get-Children $p.Id | Where-Object { $_.Name -ieq 'msedgewebview2.exe' } | Select-Object -First 1
  }
  Start-Sleep -Seconds 8
  $closed = $p.CloseMainWindow()
  $exited = $p.WaitForExit(40000)
  $code = if ($exited) { $p.ExitCode } else { 'не вышел' }
  Start-Sleep -Seconds 10 # отложенные записи после выхода (Credentials появлялся «после закрытия»)
  $info = @{ phase = $phase; pid = $p.Id; browserPid = $browser.ProcessId; browserCmd = $browser.CommandLine; closeMainWindow = $closed; exit = $code }
  Add-Content -LiteralPath $results -Value ($info | ConvertTo-Json -Compress) -Encoding utf8
}

# Любой исполняемый файл с окном WebView2: запуск, ожидание процесса браузера,
# 6 с работы, закрытие окна (зонд wry выходит сам по secs=), пауза.
function Scenario-Exe([string] $phase, [string] $exe, [string[]] $arguments, [int] $seconds = 6) {
  $p = if ($arguments) { Start-Process -FilePath $exe -ArgumentList $arguments -PassThru } else { Start-Process -FilePath $exe -PassThru }
  $null = $p.Handle
  $browser = $null
  for ($i = 0; $i -lt 60 -and -not $browser; $i++) {
    Start-Sleep -Milliseconds 500
    $browser = Get-CimInstance Win32_Process -Filter "Name = 'msedgewebview2.exe'" -ErrorAction SilentlyContinue |
      Where-Object { $_.ParentProcessId -eq $p.Id } | Select-Object -First 1
  }
  Start-Sleep -Seconds $seconds
  $closed = $false
  if (-not $p.HasExited) { $closed = $p.CloseMainWindow() }
  if (-not $p.WaitForExit(20000)) { $p.Kill() }
  Start-Sleep -Seconds 4
  $info = @{ phase = $phase; exe = $exe; args = ($arguments -join ' '); browserCmd = $browser.CommandLine; closeMainWindow = $closed }
  Add-Content -LiteralPath $results -Value ($info | ConvertTo-Json -Compress) -Encoding utf8
}

function Set-AppPolicy([string] $exeName, [string] $value) {
  $key = 'HKLM:\SOFTWARE\Policies\Microsoft\Edge\WebView2\AdditionalBrowserArguments'
  if ($value) {
    New-Item -Path $key -Force | Out-Null
    Set-ItemProperty -Path $key -Name $exeName -Value $value
  } elseif (Test-Path $key) {
    Remove-ItemProperty -Path $key -Name $exeName -ErrorAction SilentlyContinue
  }
}

function Scenario-Mini([string] $phase, [string] $mode) {
  [void] (Invoke-Probe @('webview', $mode, '8') $phase)
  Start-Sleep -Seconds 10
}

function Scenario-Dialog([string] $phase, [string] $variant, [string] $folder) {
  New-Item -ItemType Directory -Force -Path $folder | Out-Null
  [void] (Invoke-Probe @('dialog', $variant, $folder) $phase)
  Start-Sleep -Seconds 5
}

function Phase([string] $phaseName, [scriptblock] $body) {
  Write-Host "=== фаза $phaseName"
  Snap (($phaseName -replace '#', '-') + '-pre')
  Mark $phaseName 'start'
  & $body
  Mark $phaseName 'end'
  Snap (($phaseName -replace '#', '-') + '-post')
}

# --- Process Monitor -------------------------------------------------------------

$pml = Join-Path $out 'cap.pml'
Start-Process -FilePath $Procmon -ArgumentList @('/AcceptEula', '/Quiet', '/Minimized', '/BackingFile', $pml, '/LoadConfig', (Join-Path $out 't06.pmc'))
for ($i = 0; $i -lt 60 -and -not (Test-Path $pml); $i++) { Start-Sleep -Milliseconds 500 }
if (-not (Test-Path $pml)) { throw 'Process Monitor не начал запись' }
Start-Sleep -Seconds 5
Write-Host "Process Monitor пишет в $pml"

try {
  switch -Regex ($Job) {
    '^app$' {
      foreach ($i in 1..3) { Phase "idle#$i" { Scenario-Idle "idle#$i" } }
      foreach ($i in 1..5) { Phase "app#$i" { Scenario-App "app#$i" } }
    }
    '^minimal$' {
      foreach ($i in 1..3) { Phase "idle#$i" { Scenario-Idle "idle#$i" } }
      foreach ($i in 1..3) { Phase "mini-rhizomind-flags#$i" { Scenario-Mini "mini-rhizomind-flags#$i" 'rhizomind-flags' } }
      foreach ($i in 1..3) { Phase "mini-default#$i" { Scenario-Mini "mini-default#$i" 'default' } }
    }
    '^app-repeat$' {
      foreach ($i in 1..2) { Phase "idle#$i" { Scenario-Idle "idle#$i" } }
      foreach ($i in 1..5) { Phase "app#$i" { Scenario-App "app#$i" } }
    }
    '^mini-then-app$' {
      Phase 'idle#1' { Scenario-Idle 'idle#1' }
      foreach ($i in 1..3) { Phase "mini-rhizomind-flags#$i" { Scenario-Mini "mini-rhizomind-flags#$i" 'rhizomind-flags' } }
      foreach ($i in 1..3) { Phase "app#$i" { Scenario-App "app#$i" } }
    }
    '^app-then-mini$' {
      Phase 'idle#1' { Scenario-Idle 'idle#1' }
      foreach ($i in 1..3) { Phase "app#$i" { Scenario-App "app#$i" } }
      foreach ($i in 1..3) { Phase "mini-rhizomind-flags#$i" { Scenario-Mini "mini-rhizomind-flags#$i" 'rhizomind-flags' } }
    }
    '^app-policy-(.+)$' {
      # Опыт: функции движка, связанные с OneAuth/WAM, выключаются политикой WebView2
      # AdditionalBrowserArguments (без правки приложения). Список --disable-features
      # повторяет флаги Rhizomind и добавляет кандидатов: если политика заменяет
      # список приложения, защитные функции не теряются.
      $extra = @{
        'oneauthwam' = 'msOneAuthWAM'
        'background' = 'msLoadOneAuthInBackground'
        'both'       = 'msOneAuthWAM,msLoadOneAuthInBackground'
      }[$Matches[1]]
      $key = 'HKLM:\SOFTWARE\Policies\Microsoft\Edge\WebView2\AdditionalBrowserArguments'
      New-Item -Path $key -Force | Out-Null
      $value = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,$extra"
      Set-ItemProperty -Path $key -Name 'rhizomind.exe' -Value $value
      Add-Content -LiteralPath $results -Value (@{ phase = 'policy'; key = $key; value = $value } | ConvertTo-Json -Compress) -Encoding utf8
      Phase 'idle#1' { Scenario-Idle 'idle#1' }
      foreach ($i in 1..3) { Phase "app#$i" { Scenario-App "app#$i" } }
    }
    '^oneauth-sweep$' {
      # Поиск, что включает OneAuth (задача 0.6б). Признак — загрузка oneauth.dll
      # и клиента WAM процессом msedgewebview2.exe; он виден при КАЖДОМ запуске,
      # поэтому варианты идут подряд на одной машине.
      $base = '--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection'
      $dir = Split-Path $AppExe
      $root = Split-Path $dir
      # Варианты имени и места одного и того же rhizomind.exe.
      $copies = [ordered]@{
        'app-otherdir'   = Join-Path $root 'app2\rhizomind.exe'
        'app-renamed'    = Join-Path $dir 'notes-t06.exe'
        'app-upper'      = Join-Path $dir 'Rhizomind.exe.tmp'
        'app-portable'   = Join-Path $dir 'rhizomind-portable.exe'
        'app-name-app'   = Join-Path $dir 'app.exe'
        'app-name-notes' = Join-Path $dir 'notes.exe'
      }
      foreach ($name in $copies.Keys) {
        New-Item -ItemType Directory -Force -Path (Split-Path $copies[$name]) | Out-Null
        Copy-Item $AppExe $copies[$name] -Force
      }
      # Зонды под именем rhizomind.exe в папке приложения (то же место, другой код).
      $probeHome = Join-Path $root 'probe-as-app'
      Copy-Item (Split-Path $Probe) $probeHome -Recurse -Force
      $probeAsApp = Join-Path $probeHome 'rhizomind.exe'
      Copy-Item $Probe $probeAsApp -Force
      $wryAsAppDir = Join-Path $root 'wry-as-app'
      New-Item -ItemType Directory -Force -Path $wryAsAppDir | Out-Null
      $wryAsApp = Join-Path $wryAsAppDir 'rhizomind.exe'
      Copy-Item $WryProbe $wryAsApp -Force
      Phase 'idle#1' { Scenario-Idle 'idle#1' }
      Phase 'app#1' { Scenario-Exe 'app#1' $AppExe @() }
      foreach ($name in $copies.Keys) {
        $exe = $copies[$name]
        if ($name -eq 'app-upper') {
          # Регистр имени: Rhizomind.exe рядом с rhizomind.exe нельзя (NTFS без учёта регистра) — отдельная папка.
          $exe = Join-Path $root 'app3\Rhizomind.exe'
          New-Item -ItemType Directory -Force -Path (Split-Path $exe) | Out-Null
          Copy-Item $AppExe $exe -Force
        }
        Phase "$name#1" { Scenario-Exe "$name#1" $exe @() }
      }
      Phase 'app#2' { Scenario-Exe 'app#2' $AppExe @() }
      Phase 'mini-as-rhizomind#1' { Scenario-Exe 'mini-as-rhizomind#1' $probeAsApp @('webview', 'rhizomind-flags', '6') }
      Phase 'wry-as-rhizomind#1' { Scenario-Exe 'wry-as-rhizomind#1' $wryAsApp @('datadir', 'incognito', 'flags', 'protocol', 'secs=8') }
      $wryVariants = [ordered]@{
        'wry-min'       = @()
        'wry-all'       = @('datadir', 'incognito', 'flags', 'noext', 'bg', 'ipc', 'init', 'protocol')
        'wry-incognito' = @('incognito')
        'wry-datadir'   = @('datadir')
        'wry-flags'     = @('flags')
        'wry-protocol'  = @('protocol')
        'wry-ipc-init'  = @('ipc', 'init')
        'wry-bg-noext'  = @('bg', 'noext')
      }
      foreach ($name in $wryVariants.Keys) {
        $a = @($wryVariants[$name]) + 'secs=8'
        Phase "$name#1" { Scenario-Exe "$name#1" $WryProbe $a }
      }
      $policies = [ordered]@{
        'pol-enable-lazyload'   = '--enable-features=msLazyLoadOneAuthInWebView'
        'pol-no-forcewam'       = "$base,msForceOneAuthWAM"
        'pol-no-pinwam'         = "$base,msPinWAMDLLsOnWin10"
        'pol-no-wcos'           = "$base,msOneAuthWAMOnWCOS"
        'pol-no-profileaad'     = "$base,msEnableProfileAADAccountSSO"
        'pol-no-prt'            = "$base,msAllowMSAPrtSSOForNonMSAProfile,msAutoToggleMSAPrtSSOForNonMSAProfile,msAutoToggleAADPrtSSOForNonAADProfile,msShowUXForAADPrtSSOForNonAADProfile"
        'pol-no-primary'        = "$base,msEmxPrimaryAccount,msStrictPrimaryAccountCheck,msSingleSignOnOSForPrimaryAccountIsShared"
        'pol-no-ssodiag'        = "$base,msSendSSODiagnostics"
        'pol-no-all'            = "$base,msOneAuthWAM,msLoadOneAuthInBackground,msForceOneAuthWAM,msPinWAMDLLsOnWin10,msOneAuthWAMOnWCOS,msEnableProfileAADAccountSSO,msAllowMSAPrtSSOForNonMSAProfile,msAutoToggleMSAPrtSSOForNonMSAProfile,msAutoToggleAADPrtSSOForNonAADProfile,msEmxPrimaryAccount,msStrictPrimaryAccountCheck,msSendSSODiagnostics,msM365LinksImplicitSignin"
        'pol-enable-osprimaryonly' = '--enable-features=msSingleSignOnForOSPrimaryAccountOnly'
      }
      foreach ($name in $policies.Keys) {
        Set-AppPolicy 'rhizomind.exe' $policies[$name]
        Add-Content -LiteralPath $results -Value (@{ phase = "$name#1"; policy = $policies[$name] } | ConvertTo-Json -Compress) -Encoding utf8
        Phase "$name#1" { Scenario-Exe "$name#1" $AppExe @() }
      }
      Set-AppPolicy 'rhizomind.exe' ''
      Phase 'app#3' { Scenario-Exe 'app#3' $AppExe @() }
    }
    '^oneauth-long$' {
      # Загрузка oneauth.dll идёт через 3–7 с после запуска. Не успевал ли
      # минимальный WebView2 закрыться раньше? Все окна держатся 40 с.
      Phase 'idle#1' { Scenario-Idle 'idle#1' }
      Phase 'mini-flags-long#1' { Scenario-Exe 'mini-flags-long#1' $Probe @('webview', 'rhizomind-flags', '40') 42 }
      Phase 'mini-default-long#1' { Scenario-Exe 'mini-default-long#1' $Probe @('webview', 'default', '40') 42 }
      Phase 'mini-nav-lang-long#1' { Scenario-Exe 'mini-nav-lang-long#1' $Probe @('webview', 'rhizomind-flags+nav+lang', '40') 42 }
      Phase 'wry-min-long#1' { Scenario-Exe 'wry-min-long#1' $WryProbe @('secs=40') 42 }
      Phase 'app-long#1' { Scenario-Exe 'app-long#1' $AppExe @() 40 }
      Phase 'mini-flags-long#2' { Scenario-Exe 'mini-flags-long#2' $Probe @('webview', 'rhizomind-flags', '40') 42 }
    }
    '^oneauth-close$' {
      # OneAuth загружается при закрытии (≈ в момент выхода). Как закрывается окно?
      Phase 'idle#1' { Scenario-Idle 'idle#1' }
      Phase 'mini-graceful#1' { Scenario-Exe 'mini-graceful#1' $Probe @('webview', 'rhizomind-flags', '8') 12 }
      Phase 'mini-abrupt#1' { Scenario-Exe 'mini-abrupt#1' $Probe @('webview', 'rhizomind-flags+abrupt', '8') 12 }
      Phase 'mini-destroy#1' { Scenario-Exe 'mini-destroy#1' $Probe @('webview', 'rhizomind-flags+destroy', '8') 12 }
      Phase 'wry-exit#1' { Scenario-Exe 'wry-exit#1' $WryProbe @('datadir', 'incognito', 'flags', 'secs=8') 14 }
      Phase 'wry-drop#1' { Scenario-Exe 'wry-drop#1' $WryProbe @('datadir', 'incognito', 'flags', 'drop', 'secs=8') 14 }
      Phase 'wry-destroy#1' { Scenario-Exe 'wry-destroy#1' $WryProbe @('datadir', 'incognito', 'flags', 'destroy', 'secs=8') 14 }
      Phase 'app#1' { Scenario-Exe 'app#1' $AppExe @() 8 }
      Phase 'mini-abrupt#2' { Scenario-Exe 'mini-abrupt#2' $Probe @('webview', 'rhizomind-flags+abrupt', '8') 12 }
      Phase 'wry-drop#2' { Scenario-Exe 'wry-drop#2' $WryProbe @('datadir', 'incognito', 'flags', 'drop', 'secs=8') 14 }
      Phase 'mini-graceful#2' { Scenario-Exe 'mini-graceful#2' $Probe @('webview', 'rhizomind-flags', '8') 12 }
      Phase 'wry-exit#2' { Scenario-Exe 'wry-exit#2' $WryProbe @('datadir', 'incognito', 'flags', 'secs=8') 14 }
    }
    '^oneauth-dpi$' {
      # Отличие командной строки C# и wry: --embedded-browser-webview-dpi-awareness (0 против 2) и --lang.
      Phase 'idle#1' { Scenario-Idle 'idle#1' }
      Phase 'mini#1' { Scenario-Exe 'mini#1' $Probe @('webview', 'rhizomind-flags', '8') 12 }
      Phase 'mini-dpi#1' { Scenario-Exe 'mini-dpi#1' $Probe @('webview', 'rhizomind-flags+dpi', '8') 12 }
      Phase 'mini-lang#1' { Scenario-Exe 'mini-lang#1' $Probe @('webview', 'rhizomind-flags+lang', '8') 12 }
      Phase 'mini-dpi-lang#1' { Scenario-Exe 'mini-dpi-lang#1' $Probe @('webview', 'rhizomind-flags+dpi+lang', '8') 12 }
      Phase 'wry#1' { Scenario-Exe 'wry#1' $WryProbe @('datadir', 'incognito', 'flags', 'drop', 'secs=8') 14 }
      Phase 'mini-dpi#2' { Scenario-Exe 'mini-dpi#2' $Probe @('webview', 'rhizomind-flags+dpi', '8') 12 }
      Phase 'mini#2' { Scenario-Exe 'mini#2' $Probe @('webview', 'rhizomind-flags', '8') 12 }
      Phase 'mini-dpi-lang#2' { Scenario-Exe 'mini-dpi-lang#2' $Probe @('webview', 'rhizomind-flags+dpi+lang', '8') 12 }
      Phase 'app#1' { Scenario-Exe 'app#1' $AppExe @() 8 }
    }
    '^oneauth-api$' {
      # Командная строка одинакова; отличие — вызовы API окна. Вызовы wry по одному в C#.
      $m = 'rhizomind-flags+dpi+lang'
      Phase 'idle#1' { Scenario-Idle 'idle#1' }
      Phase 'mini#1' { Scenario-Exe 'mini#1' $Probe @('webview', $m, '8') 12 }
      Phase 'mini-wry#1' { Scenario-Exe 'mini-wry#1' $Probe @('webview', "$m+wry", '8') 12 }
      foreach ($v in @('ncr', 'xdrop', 'settings', 'events', 'script')) {
        Phase "mini-$v#1" { Scenario-Exe "mini-$v#1" $Probe @('webview', "$m+$v", '8') 12 }
      }
      Phase 'mini-wry#2' { Scenario-Exe 'mini-wry#2' $Probe @('webview', "$m+wry", '8') 12 }
      Phase 'wry#1' { Scenario-Exe 'wry#1' $WryProbe @('datadir', 'incognito', 'flags', 'drop', 'secs=8') 14 }
    }
    '^oneauth-sdk$' {
      # Оставшееся отличие — сторона хоста: версия SDK/загрузчика и объект параметров окружения.
      $m = 'rhizomind-flags+dpi+lang+wry+noext'
      $probe3800 = Join-Path (Split-Path (Split-Path $Probe)) 't06probe-3800\t06probe.exe'
      Phase 'idle#1' { Scenario-Idle 'idle#1' }
      Phase 'mini#1' { Scenario-Exe 'mini#1' $Probe @('webview', $m, '8') 12 }
      Phase 'mini-tcbv#1' { Scenario-Exe 'mini-tcbv#1' $Probe @('webview', "$m+tcbv", '8') 12 }
      Phase 'mini3800#1' { Scenario-Exe 'mini3800#1' $probe3800 @('webview', $m, '8') 12 }
      Phase 'mini3800-tcbv#1' { Scenario-Exe 'mini3800-tcbv#1' $probe3800 @('webview', "$m+tcbv", '8') 12 }
      Phase 'wry#1' { Scenario-Exe 'wry#1' $WryProbe @('datadir', 'incognito', 'flags', 'drop', 'secs=8') 14 }
      Phase 'mini3800#2' { Scenario-Exe 'mini3800#2' $probe3800 @('webview', $m, '8') 12 }
      Phase 'mini-tcbv#2' { Scenario-Exe 'mini-tcbv#2' $Probe @('webview', "$m+tcbv", '8') 12 }
    }
    '^mini(-nav|-lang|-nav-lang)$' {
      $tag = $Matches[1]
      $mode = 'rhizomind-flags' + ($tag -replace '-', '+')
      Phase 'idle#1' { Scenario-Idle 'idle#1' }
      foreach ($i in 1..2) { Phase "mini$tag#$i" { Scenario-Mini "mini$tag#$i" $mode } }
      Phase 'app#1' { Scenario-App 'app#1' }
    }
    '^dialog-(.+)$' {
      $variant = $Matches[1]
      Phase 'idle#1' { Scenario-Idle 'idle#1' }
      foreach ($i in 1..3) {
        $folder = Join-Path ([Environment]::GetFolderPath('MyDocuments')) "t06-vault-$variant-$i"
        Phase "dialog-$variant#$i" { Scenario-Dialog "dialog-$variant#$i" $variant $folder }
      }
    }
    default { throw "неизвестное задание $Job" }
  }
}
finally {
  $markers | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $out 'markers.json') -Encoding utf8
  Start-Process -FilePath $Procmon -ArgumentList '/Terminate' -Wait
  Get-ChildItem -LiteralPath $out -Filter 'cap*.pml' | ForEach-Object { Write-Host "$($_.Name): $($_.Length) байт" }
}

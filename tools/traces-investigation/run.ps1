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
  [string] $AppExe = ''
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

function Scenario-Mini([string] $phase, [string] $mode) {
  [void] (Invoke-Probe @('webview', $mode, '8') $phase)
  Start-Sleep -Seconds 10
}

function Scenario-Dialog([string] $phase, [string] $variant, [string] $folder) {
  New-Item -ItemType Directory -Force -Path $folder | Out-Null
  [void] (Invoke-Probe @('dialog', $variant, $folder) $phase)
  Start-Sleep -Seconds 5
}

function Phase([string] $name, [scriptblock] $body) {
  Write-Host "=== фаза $name"
  Snap (($name -replace '#', '-') + '-pre')
  Mark $name 'start'
  & $body
  Mark $name 'end'
  Snap (($name -replace '#', '-') + '-post')
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

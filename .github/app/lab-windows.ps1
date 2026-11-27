param(
    [Parameter(Mandatory = $true)][string]$Built,
    [Parameter(Mandatory = $true)][string]$Out
)

$ErrorActionPreference = 'Continue'
New-Item -ItemType Directory -Force $Out | Out-Null
$report = Join-Path $Out 'report.md'
Set-Content -Path $report -Value ''
$folder = Join-Path $env:LOCALAPPDATA 'Programs\Scooch'
$data = Join-Path $env:LOCALAPPDATA 'Scooch'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes

function Line([string]$Text) { Add-Content -Path $report -Value $Text; Write-Host $Text }
function Wait-For([int]$Seconds, [scriptblock]$Test) {
    $started = Get-Date
    while (((Get-Date) - $started).TotalSeconds -lt $Seconds) {
        if (& $Test) { return [int]((Get-Date) - $started).TotalSeconds }
        Start-Sleep -Milliseconds 500
    }
    return -1
}
function Find-Window {
    $root = [System.Windows.Automation.AutomationElement]::RootElement
    $byName = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, 'Scooch')
    return $root.FindFirst([System.Windows.Automation.TreeScope]::Children, $byName)
}
function Read-Shared([string]$Path) {
    try {
        $stream = [System.IO.File]::Open($Path, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]'ReadWrite, Delete')
        try { return (New-Object System.IO.StreamReader($stream)).ReadToEnd() } finally { $stream.Dispose() }
    } catch { return '' }
}
function State-On { (Test-Path "$data\state.json") -and ((Read-Shared "$data\state.json") -match '"on": true') }
function Icon-Processes { @(Get-Process -Name 'scooch-app' -ErrorAction SilentlyContinue) }
function Megabytes([long]$Bytes) { [math]::Round($Bytes / 1MB, 1) }
function Kind($Process) {
    $said = [string]$Process.CommandLine
    if ($Process.Name -ne 'msedgewebview2.exe') { return '' }
    if ($said -match '--type=([a-z-]+)') {
        $kind = $Matches[1]
        if ($said -match '--utility-sub-type=([A-Za-z0-9_.]+)') { $kind = "$kind $($Matches[1])" }
        return " as $kind"
    }
    return ' as the browser process'
}
function Held($Process) {
    $live = Get-Process -Id ([int]$Process.ProcessId) -ErrorAction SilentlyContinue
    if (-not $live) { return $null }
    return @([long]$live.PrivateMemorySize64, [long]$live.WorkingSet64, [int]$live.Threads.Count)
}
function Below([int]$Root, $All) {
    $found = @()
    $queue = New-Object System.Collections.Queue
    $queue.Enqueue($Root)
    $seen = @{ $Root = $true }
    while ($queue.Count -gt 0) {
        $parent = [int]$queue.Dequeue()
        $born = ($All | Where-Object { [int]$_.ProcessId -eq $parent } | Select-Object -First 1).CreationDate
        foreach ($child in @($All | Where-Object { [int]$_.ParentProcessId -eq $parent })) {
            $id = [int]$child.ProcessId
            if ($seen.ContainsKey($id)) { continue }
            if ($born -and $child.CreationDate -and $child.CreationDate -lt $born) { continue }
            $seen[$id] = $true
            $found += $child
            $queue.Enqueue($id)
        }
    }
    return $found
}
function Snap([string]$Label) {
    Line ''
    Line "#### $Label"
    $icons = Icon-Processes
    if ($icons.Count -eq 0) { Line '- no icon process runs'; return }
    $all = @(Get-CimInstance Win32_Process)
    $rows = @()
    foreach ($icon in $icons) {
        $icon.Refresh()
        Line "- the icon process scooch-app $($icon.Id) holds $(Megabytes $icon.PrivateMemorySize64) MB private with a working set of $(Megabytes $icon.WorkingSet64) MB, $($icon.Threads.Count) threads"
        $rows += [pscustomobject]@{ label = $Label; name = 'scooch-app.exe'; pid = $icon.Id; parent = 0; kind = 'icon'; private = [long]$icon.PrivateMemorySize64; working = [long]$icon.WorkingSet64 }
        $under = @(Below $icon.Id $all)
        $private = 0
        $working = 0
        foreach ($child in $under) {
            $held = Held $child
            if (-not $held) { Line "- below the icon process: $($child.Name) $($child.ProcessId) with parent $($child.ParentProcessId) ended while it was read"; continue }
            $private += $held[0]
            $working += $held[1]
            Line "- below the icon process: $($child.Name) $($child.ProcessId)$(Kind $child) with parent $($child.ParentProcessId), $(Megabytes $held[0]) MB private with a working set of $(Megabytes $held[1]) MB, $($held[2]) threads"
            $rows += [pscustomobject]@{ label = $Label; name = $child.Name; pid = [int]$child.ProcessId; parent = [int]$child.ParentProcessId; kind = (Kind $child).Trim(); private = $held[0]; working = $held[1] }
        }
        if ($under.Count -eq 0) { Line '- below the icon process: no process' }
        else { Line "- the $($under.Count) processes below the icon process hold $(Megabytes $private) MB private with working sets of $(Megabytes $working) MB together, and with the icon process $(Megabytes ($private + $icon.PrivateMemorySize64)) MB private" }
        $known = @($icon.Id) + @($under | ForEach-Object { [int]$_.ProcessId })
        foreach ($other in @($all | Where-Object { $known -notcontains [int]$_.ProcessId -and (($_.Name -in @('scoochw.exe', 'scooch.exe')) -or ($_.Name -eq 'msedgewebview2.exe' -and [string]$_.CommandLine -match 'scooch')) })) {
            $held = Held $other
            if (-not $held) { continue }
            Line "- outside the icon process's tree: $($other.Name) $($other.ProcessId)$(Kind $other) with parent $($other.ParentProcessId), $(Megabytes $held[0]) MB private with a working set of $(Megabytes $held[1]) MB"
            $rows += [pscustomobject]@{ label = $Label; name = $other.Name; pid = [int]$other.ProcessId; parent = [int]$other.ParentProcessId; kind = 'outside ' + (Kind $other).Trim(); private = $held[0]; working = $held[1] }
        }
    }
    $rows | ForEach-Object { Add-Content -Path (Join-Path $Out 'data.jsonl') -Value ($_ | ConvertTo-Json -Compress) }
}

$os = Get-CimInstance Win32_OperatingSystem
Line "## Memory lab on $($os.Caption) $($os.Version), the processes below the icon process"
Line ''
$setup = Get-ChildItem $Built -Filter '*-setup.exe' | Select-Object -First 1
$took = Measure-Command { (Start-Process -FilePath $setup.FullName -ArgumentList '/S' -PassThru).WaitForExit(300000) | Out-Null }
Line "- silent install of $($setup.Name) took $([int]$took.TotalSeconds) s into $folder holding $((Get-ChildItem $folder -File | ForEach-Object Name) -join ', ')"
$version = (Get-ItemProperty 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}' -ErrorAction SilentlyContinue).pv
Line "- the WebView2 runtime is $version"
$photo = Join-Path (Resolve-Path 'assets/photos').Path 'golden-tree.jpg'
$empty = Join-Path $Out 'empty.txt'
Set-Content -Path $empty -Value ''
$command = Start-Process -FilePath (Join-Path $folder 'scooch.exe') -ArgumentList 'use', $photo -NoNewWindow -PassThru -RedirectStandardInput $empty -RedirectStandardOutput (Join-Path $Out 'use.txt') -RedirectStandardError (Join-Path $Out 'use-error.txt')
if (-not $command.WaitForExit(60000)) { Stop-Process -Id $command.Id -Force -ErrorAction SilentlyContinue }
$on = Wait-For 40 { State-On }
$drawn = Wait-For 40 { (Test-Path "$data\state.json") -and ((Read-Shared "$data\state.json") -match '"fingerprint": "') }
$own = Wait-For 30 { (Icon-Processes).Count -ge 1 }
Line "- scooch use turned Scooch on after $on s and drew after $drawn s, daemons $(@(Get-Process -Name scoochw -ErrorAction SilentlyContinue).Count), the daemon's own icon process came after $own s"
Start-Sleep -Seconds 5
Snap 'started by the daemon with no window ever'
Icon-Processes | Stop-Process -Force -ErrorAction SilentlyContinue
Wait-For 10 { (Icon-Processes).Count -eq 0 } | Out-Null
$log = Join-Path $Out 'app.log'
Start-Process -FilePath (Join-Path $folder 'scooch-app.exe') -RedirectStandardError $log
$shown = Wait-For 120 { [bool](Find-Window) }
Line ''
Line "- a start of scooch-app showed its window after $shown s, app processes $((Icon-Processes).Count)"
Start-Sleep -Seconds 20
Snap 'window open for 20 s'
$window = Find-Window
if ($window) { $window.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).Close() }
$closed = Get-Date
$gone = Wait-For 15 { -not (Find-Window) }
Line ''
Line "- closed the window, gone after $gone s, app processes $((Icon-Processes).Count), the app says $((Read-Shared $log) -replace "`r?`n", ' ')"
foreach ($mark in 10, 60, 300) {
    while (((Get-Date) - $closed).TotalSeconds -lt $mark) { Start-Sleep -Milliseconds 500 }
    Snap "$mark s after the close"
}
Get-Process -Name 'scooch-app', 'scoochw' -ErrorAction SilentlyContinue | Stop-Process -Force
exit 0

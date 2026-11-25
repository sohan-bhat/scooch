Line ''
Line '#### Experiment, the daemon started again while the icon runs, many times'
$rounds = 30
$restartLog = Join-Path $Out 'restarts.log'
$startTrace = $false
try {
    Register-CimIndicationEvent -Namespace 'root/cimv2' -Query "SELECT * FROM Win32_ProcessStartTrace WHERE ProcessName = 'scooch-app.exe' OR ProcessName = 'scoochw.exe'" -SourceIdentifier 'ProbeStarts' -ErrorAction Stop
    $startTrace = $true
} catch { Line "- the system's process start trace could not be opened, $($_.Exception.Message)" }
function Trace-Starts {
    if (-not $startTrace) { return 'not open' }
    $events = @(Get-Event -SourceIdentifier 'ProbeStarts' -ErrorAction SilentlyContinue)
    $events | Remove-Event -ErrorAction SilentlyContinue
    (@($events | ForEach-Object { $e = $_.SourceEventArgs.NewEvent; "$($e.ProcessName) $($e.ProcessID) made by $($e.ParentProcessID) at $([DateTimeOffset]::FromFileTime([long]$e.TIME_CREATED).ToUnixTimeMilliseconds())" }) -join '; ')
}
$misses = @()
$tally = @{}
for ($round = 1; $round -le $rounds; $round++) {
    $kind = @('redirected', 'plain', 'traced')[$round % 3]
    $old = @(Get-Process -Name scoochw -ErrorAction SilentlyContinue | ForEach-Object Id)
    Get-Process -Name scoochw -ErrorAction SilentlyContinue | Stop-Process -Force
    $gone = Wait-For 15 { (Daemons) -eq 0 }
    Start-Sleep -Seconds 3
    $early = @(Get-CimInstance Win32_Process -Filter "Name = 'scoochw.exe'" -ErrorAction SilentlyContinue | ForEach-Object { "$($_.ProcessId) made by $($_.ParentProcessId)" }) -join ', '
    $before = Trace-Starts
    $known = [int[]](@(Icon-Processes | ForEach-Object Id) + $old)
    $log = if ($kind -eq 'redirected') { Join-Path $Out "restart-$round-stderr.log" } else { $daemonLog }
    $from = if ($kind -eq 'redirected') { [long]0 } else { Log-Length $daemonLog }
    if ($kind -eq 'traced') { $env:SCOOCH_TRACE = '1' }
    $at = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    if ($kind -eq 'redirected') {
        $started = Start-Process -FilePath $program -ArgumentList $rest -PassThru -RedirectStandardError $log
    } else {
        $started = Start-Process -FilePath $program -ArgumentList $rest -WindowStyle Hidden -PassThru
    }
    $issued = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() - $at
    Remove-Item Env:SCOOCH_TRACE -ErrorAction SilentlyContinue
    $watched = [Tray]::Watch([string[]]@('scooch-app.exe', 'scoochw.exe'), $known, $log, $from, 30000, 3000) -split '\|', 4
    Start-Sleep -Milliseconds 500
    $traced = Trace-Starts
    $code = if ($started.HasExited) { "ended with code $($started.ExitCode)" } else { 'runs' }
    $born = try { [DateTimeOffset]::new($started.StartTime.ToUniversalTime()).ToUnixTimeMilliseconds() - $at } catch { 'unknown' }
    $lines = @((Read-After $log $from) -split "`r?`n" | Where-Object { $_ })
    $entries = @($watched[3] -split ';' | Where-Object { $_ })
    $child = $entries | Where-Object { $_ -match "^scooch-app\.exe (\d+) $($started.Id) (\d+) (\d+)$" } | Select-Object -First 1
    $childId = 0
    $childAt = -1
    if ($child -and $child -match '^scooch-app\.exe (\d+) \d+ (\d+) (\d+)$') { $childId = [int]$Matches[1]; $childAt = [int]$Matches[2] }
    $iconLine = $lines | Where-Object { $_ -match ' icon (not )?started ' } | Select-Object -First 1
    $iconAt = if ($iconLine -match '^(\d+)\.(\d{3}) ') { [long]$Matches[1] * 1000 + [long]$Matches[2] - $at } else { 'never' }
    $kept = if ($childId) { [Tray]::Object($childId) } else { 'no child seen' }
    $miss = $childAt -lt 0 -or $childAt -gt 8000
    $verdict = if ($miss) { 'MISS' } else { 'seen' }
    $tally["$kind $verdict"] = 1 + [int]$tally["$kind $verdict"]
    Line "- round $round $kind $verdict, killed $($old -join ' ') gone after $gone s, scoochw before the start $(if ($early) { $early } else { 'none' }), start trace before the start $(if ($before) { $before } else { 'none' }), Start-Process returned after $issued ms, the daemon $($started.Id) was made $born ms after the start and $code, its icon line came $iconAt ms after the start, the log was heard at $($watched[0]) ms of the watch, $($watched[1]) snapshots in $($watched[2]) ms, new processes as name id maker first last $($entries -join '; '), the daemon's child $(if ($childId) { "$childId first seen at $childAt ms, now $kept" } else { 'none' }), the start trace says $(if ($traced) { $traced } else { 'nothing' }), app processes now $(Icon-Count)"
    Line "  - the daemon's lines $(@($lines | Select-Object -First 12) -join ' | ')"
    Add-Content -Path $restartLog -Value "== round $round $kind $verdict at $at, daemon $($started.Id)"
    Add-Content -Path $restartLog -Value ($lines -join "`n")
    if ($miss) { $misses += $round }
}
if ($startTrace) { Unregister-Event -SourceIdentifier 'ProbeStarts' -ErrorAction SilentlyContinue }
Line "- experiment, $rounds restarts, misses within 8 s $($misses.Count) in rounds $($misses -join ' '), by kind $(@($tally.Keys | Sort-Object | ForEach-Object { "$_ $($tally[$_])" }) -join ', ')"

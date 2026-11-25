Line ''
Line '#### Experiment, the daemon started again while the icon runs, many times'
$rounds = 100
$restartLog = Join-Path $Out 'restarts.log'
$startTrace = $false
try {
    Register-CimIndicationEvent -Namespace 'root/cimv2' -Query "SELECT * FROM Win32_ProcessStartTrace WHERE ProcessName = 'scooch-app.exe' OR ProcessName = 'scoochw.exe'" -SourceIdentifier 'ProbeStarts' -ErrorAction Stop
    $startTrace = $true
} catch { Line "- the system's process start trace could not be opened, $($_.Exception.Message)" }
function Trace-Starts {
    if (-not $startTrace) { return @() }
    $events = @(Get-Event -SourceIdentifier 'ProbeStarts' -ErrorAction SilentlyContinue)
    $events | Remove-Event -ErrorAction SilentlyContinue
    @($events | ForEach-Object { $e = $_.SourceEventArgs.NewEvent; "$($e.ProcessName) $($e.ProcessID) $($e.ParentProcessID)" })
}
$tally = @{ misses = 0; traced = 0; once = 0; twice = 0; more = 0; lateLine = 0; extraDaemon = 0; extraApp = 0 }
$missed = @()
$lives = @()
$middles = @()
$mosts = @()
$icons = @()
for ($round = 1; $round -le $rounds; $round++) {
    $old = @(Get-Process -Name scoochw -ErrorAction SilentlyContinue | ForEach-Object Id)
    Get-Process -Name scoochw -ErrorAction SilentlyContinue | Stop-Process -Force
    $gone = Wait-For 15 { (Daemons) -eq 0 }
    Start-Sleep -Seconds 3
    $early = @(Get-Process -Name scoochw -ErrorAction SilentlyContinue).Count
    $null = Trace-Starts
    $known = [int[]](@(Icon-Processes | ForEach-Object Id) + $old)
    $from = Log-Length $daemonLog
    $at = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    $started = Start-Process -FilePath $program -ArgumentList $rest -WindowStyle Hidden -PassThru
    $measured = [Tray]::Measure($known, $daemonLog, $from, 30000, 2000) -split '\|', 6
    Start-Sleep -Milliseconds 700
    $traced = @(Trace-Starts)
    $lines = @((Read-After $daemonLog $from) -split "`r?`n" | Where-Object { $_ })
    $iconLine = $lines | Where-Object { $_ -match ' icon (not )?started ' } | Select-Object -First 1
    $iconAt = if ($iconLine -match '^(\d+)\.(\d{3}) ') { [long]$Matches[1] * 1000 + [long]$Matches[2] - $at } else { -1 }
    $icons += $iconAt
    if ($iconAt -lt 0 -or $iconAt -gt 8000) { $tally.lateLine++ }
    if ($early -gt 0) { $tally.extraDaemon++ }
    $children = @($traced | Where-Object { $_ -match "^scooch-app\.exe (\d+) $($started.Id)$" })
    $others = @($traced | Where-Object { $_ -match '^scooch-app\.exe ' -and $_ -notmatch " $($started.Id)$" })
    $daemonsStarted = @($traced | Where-Object { $_ -match '^scoochw\.exe ' -and $_ -notmatch "^scoochw\.exe $($started.Id) " })
    if ($others.Count -gt 0 -or $daemonsStarted.Count -gt 0) { $tally.extraApp++ }
    if ($children.Count -gt 0) { $tally.traced++ }
    $seen = @($measured[5] -split ';' | Where-Object { $_ })
    $child = $seen | Where-Object { $_ -match "^\d+ $($started.Id) " } | Select-Object -First 1
    $kept = 'none'
    if ($child) {
        $parts = $child -split ' '
        $times = [int]$parts[3]
        if ($times -eq 1) { $tally.once++ } elseif ($times -eq 2) { $tally.twice++ } else { $tally.more++ }
        if ($parts.Count -ge 6) { $lives += [double]$parts[4] }
        $kept = [Tray]::Object([int]$parts[0])
    } else {
        $tally.misses++
        $missed += $round
    }
    $middles += [double]$measured[3]
    $mosts += [double]$measured[4]
    $verdict = if ($child) { 'seen' } else { 'MISS' }
    Line "- round $round $verdict, the daemon $($started.Id) $(if ($started.HasExited) { "ended with code $($started.ExitCode)" } else { 'runs' }), scoochw before the start $early, its icon line $iconAt ms after the start, snapshots $($measured[1]) in $($measured[2]) ms with a median period of $($measured[3]) ms and the longest $($measured[4]) ms, polled children as id maker first-ms times-seen life-ms captures-in-life $($seen -join '; '), the start trace says $($traced -join '; '), child handle $kept, app processes $(Icon-Count)"
    if (-not $child) { Line "  - the daemon's lines $($lines -join ' | ')" }
    Add-Content -Path $restartLog -Value "== round $round $verdict at $at, daemon $($started.Id), trace $($traced -join '; '), polled $($seen -join '; ')"
    Add-Content -Path $restartLog -Value ($lines -join "`n")
}
if ($startTrace) { Unregister-Event -SourceIdentifier 'ProbeStarts' -ErrorAction SilentlyContinue }
function Spread($Values) { $sorted = @($Values | Sort-Object); if ($sorted.Count -eq 0) { return 'none' }; "least $($sorted[0]), median $($sorted[[int]($sorted.Count / 2)]), most $($sorted[-1])" }
Line "- experiment, $rounds restarts, the poll missed the daemon's child in $($tally.misses) rounds $($missed -join ' '), the start trace saw the daemon's child in $($tally.traced), the poll saw it in one snapshot $($tally.once), two $($tally.twice), more $($tally.more), a daemon before the start in $($tally.extraDaemon), another start in the trace in $($tally.extraApp), no icon line within 8 s in $($tally.lateLine)"
Line "- the icon line came after the start in ms $(Spread $icons), the child lived in ms $(Spread $lives), the poll's median period in ms $(Spread $middles), its longest period in ms $(Spread $mosts)"

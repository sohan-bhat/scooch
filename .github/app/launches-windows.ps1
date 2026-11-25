Add-Type @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Runtime.InteropServices;
public static class Launches {
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct Entry {
        public uint Size;
        public uint Usage;
        public uint Id;
        public IntPtr Heap;
        public uint Module;
        public uint Threads;
        public uint Parent;
        public int Priority;
        public uint Flags;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 260)] public string Name;
    }
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern IntPtr CreateToolhelp32Snapshot(uint flags, uint pid);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
    static extern bool Process32FirstW(IntPtr snapshot, ref Entry entry);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
    static extern bool Process32NextW(IntPtr snapshot, ref Entry entry);
    [DllImport("kernel32.dll")]
    static extern bool CloseHandle(IntPtr handle);
    [DllImport("kernel32.dll")]
    static extern IntPtr OpenProcess(uint access, bool inherit, int pid);
    [DllImport("kernel32.dll")]
    static extern bool GetProcessTimes(IntPtr process, out long creation, out long exit, out long kernel, out long user);
    [DllImport("kernel32.dll")]
    static extern bool GetExitCodeProcess(IntPtr process, out uint code);
    delegate bool EnumProc(IntPtr window, IntPtr extra);
    [DllImport("user32.dll")]
    static extern bool EnumWindows(EnumProc callback, IntPtr extra);
    [DllImport("user32.dll")]
    static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    static extern int GetWindowTextW(IntPtr window, System.Text.StringBuilder text, int count);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    static extern int GetClassNameW(IntPtr window, System.Text.StringBuilder text, int count);
    [DllImport("user32.dll")]
    static extern uint GetWindowThreadProcessId(IntPtr window, out uint pid);
    [DllImport("dwmapi.dll")]
    static extern int DwmGetWindowAttribute(IntPtr window, int attribute, out int value, int size);

    public static long Now() { return (DateTime.UtcNow.ToFileTimeUtc() - 116444736000000000L) / 10000; }
    static long Epoch(long filetime) { return (filetime - 116444736000000000L) / 10000; }

    class Tail {
        public string Path;
        public long Position;
        public string Partial = "";
        public void Read(long now, List<string> told) {
            try {
                using (var stream = new System.IO.FileStream(Path, System.IO.FileMode.Open, System.IO.FileAccess.Read, System.IO.FileShare.ReadWrite | System.IO.FileShare.Delete)) {
                    if (stream.Length <= Position) return;
                    stream.Seek(Position, System.IO.SeekOrigin.Begin);
                    var bytes = new byte[stream.Length - Position];
                    int got = stream.Read(bytes, 0, bytes.Length);
                    Position += got;
                    Partial += System.Text.Encoding.UTF8.GetString(bytes, 0, got);
                    int at;
                    while ((at = Partial.IndexOf('\n')) >= 0) {
                        told.Add("line " + now + " " + Partial.Substring(0, at).TrimEnd('\r'));
                        Partial = Partial.Substring(at + 1);
                    }
                }
            } catch { }
        }
    }

    public static string Watch(string log, int cap, int after) {
        var clock = Stopwatch.StartNew();
        var told = new List<string>();
        var handles = new Dictionary<uint, IntPtr>();
        var parents = new Dictionary<uint, uint>();
        var seen = new Dictionary<uint, long>();
        var views = new HashSet<uint>();
        var windows = new Dictionary<IntPtr, string>();
        var shownOnes = new HashSet<IntPtr>();
        var tail = new Tail { Path = log };
        long shown = -1;
        long read = 0;
        long polls = 0;
        while (clock.ElapsedMilliseconds < cap && (shown < 0 || Now() < shown + after)) {
            polls++;
            long now = Now();
            IntPtr snapshot = CreateToolhelp32Snapshot(2, 0);
            var entry = new Entry();
            entry.Size = (uint)Marshal.SizeOf(typeof(Entry));
            if (Process32FirstW(snapshot, ref entry)) {
                do {
                    if (string.Equals(entry.Name, "scooch-app.exe", StringComparison.OrdinalIgnoreCase)) {
                        if (!seen.ContainsKey(entry.Id)) {
                            seen[entry.Id] = now;
                            parents[entry.Id] = entry.Parent;
                            handles[entry.Id] = OpenProcess(0x1000, false, (int)entry.Id);
                        }
                    } else if (string.Equals(entry.Name, "msedgewebview2.exe", StringComparison.OrdinalIgnoreCase) && seen.ContainsKey(entry.Parent) && views.Add(entry.Id)) {
                        told.Add("webview " + entry.Id + " of " + entry.Parent + " seen " + now);
                    }
                } while (Process32NextW(snapshot, ref entry));
            }
            CloseHandle(snapshot);
            EnumWindows((window, extra) => {
                uint pid;
                GetWindowThreadProcessId(window, out pid);
                if (!seen.ContainsKey(pid)) return true;
                var name = new System.Text.StringBuilder(256);
                GetWindowTextW(window, name, 256);
                if (!windows.ContainsKey(window)) {
                    var kind = new System.Text.StringBuilder(256);
                    GetClassNameW(window, kind, 256);
                    windows[window] = pid + " '" + name + "' of class " + kind;
                    told.Add("window " + windows[window] + " made " + now);
                }
                int cloaked = 0;
                DwmGetWindowAttribute(window, 14, out cloaked, 4);
                if (IsWindowVisible(window) && cloaked == 0 && shownOnes.Add(window)) {
                    told.Add("window " + windows[window] + " shown " + now + " as '" + name + "'");
                    if (shown < 0 && name.ToString() == "Scooch") shown = now;
                }
                return true;
            }, IntPtr.Zero);
            if (now >= read) {
                read = now + 40;
                tail.Read(now, told);
            }
            System.Threading.Thread.Sleep(10);
        }
        tail.Read(Now(), told);
        foreach (var pair in handles) {
            string made = "unknown", ended = "unknown";
            if (pair.Value != IntPtr.Zero) {
                long creation, exit, kernel, user;
                uint code;
                if (GetProcessTimes(pair.Value, out creation, out exit, out kernel, out user)) {
                    made = Epoch(creation).ToString();
                    ended = GetExitCodeProcess(pair.Value, out code) && code == 259 ? "running" : Epoch(exit).ToString();
                }
                CloseHandle(pair.Value);
            }
            told.Add("app " + pair.Key + " of " + parents[pair.Key] + " made " + made + " seen " + seen[pair.Key] + " ended " + ended);
        }
        told.Add("shown " + shown);
        told.Add("polls " + polls + " in " + clock.ElapsedMilliseconds);
        return string.Join("\n", told);
    }
}
'@

$script:launchLog = Join-Path $Out 'launches.log'
$script:launchFolder = Join-Path $Out 'launches'
New-Item -ItemType Directory -Force $script:launchFolder | Out-Null

function Read-Watch([string]$Text, [int]$Launched) {
    $found = @{ apps = @(); lines = @(); windows = @(); views = @(); shown = -1; made = 0 }
    foreach ($row in ($Text -split "`n")) {
        if ($row -match '^app (\d+) of (\d+) made (\d+|unknown) seen (\d+) ended (\d+|running|unknown)$') {
            $found.apps += [pscustomobject]@{ Id = [int]$Matches[1]; Parent = [int]$Matches[2]; Made = $(if ($Matches[3] -eq 'unknown') { [long]$Matches[4] } else { [long]$Matches[3] }); Ended = $Matches[5] }
        } elseif ($row -match '^line (\d+) (.*)$') {
            $found.lines += [pscustomobject]@{ At = [long]$Matches[1]; Text = $Matches[2] }
        } elseif ($row -match '^window (\d+) ''(.*)'' of class (.*) (made|shown) (\d+)') {
            $found.windows += [pscustomobject]@{ Id = [int]$Matches[1]; Name = $Matches[2]; Kind = $Matches[3]; What = $Matches[4]; At = [long]$Matches[5] }
        } elseif ($row -match '^webview (\d+) of (\d+) seen (\d+)$') {
            $found.views += [pscustomobject]@{ Id = [int]$Matches[1]; Parent = [int]$Matches[2]; At = [long]$Matches[3] }
        } elseif ($row -match '^shown (-?\d+)$') {
            $found.shown = [long]$Matches[1]
        }
    }
    $mine = $found.apps | Where-Object { $_.Id -eq $Launched } | Select-Object -First 1
    $found.made = if ($mine) { $mine.Made } else { [Launches]::Now() }
    return $found
}

function Timeline($Found, [int]$Launched, [int]$Daemon) {
    $zero = $Found.made
    $parts = @()
    foreach ($one in $Found.apps) {
        $who = if ($one.Id -eq $Launched) { 'the launch' } elseif ($one.Parent -eq $Daemon) { 'the daemon''s start' } else { "a start by $($one.Parent)" }
        $end = if ($one.Ended -match '^\d+$') { "ended at $([long]$one.Ended - $zero)" } else { $one.Ended }
        $parts += "$who $($one.Id) made at $($one.Made - $zero) $end"
    }
    foreach ($window in $Found.windows) { $parts += "$($window.Id) '$($window.Name)' $($window.Kind) $($window.What) at $($window.At - $zero)" }
    foreach ($view in $Found.views) { $parts += "webview $($view.Id) of $($view.Parent) at $($view.At - $zero)" }
    foreach ($said in $Found.lines) { $parts += "said '$($said.Text)' at $($said.At - $zero)" }
    return $parts -join '; '
}

function UIA-Lag {
    $clock = [Diagnostics.Stopwatch]::StartNew()
    while ($clock.ElapsedMilliseconds -lt 10000) {
        if (Find-Window) { return [int]$clock.ElapsedMilliseconds }
        Start-Sleep -Milliseconds 100
    }
    return -1
}

function First-Launch($Started) {
    $text = [Launches]::Watch((Join-Path $Out 'app.log'), 120000, 1000)
    Add-Content -Path $script:launchLog -Value "== first launch $($Started.Id)`n$text"
    $found = Read-Watch $text $Started.Id
    $shownAt = if ($found.shown -ge 0) { $found.shown - $found.made } else { -1 }
    $lag = if ($found.shown -ge 0) { UIA-Lag } else { -1 }
    Line "- experiment, the first launch $($Started.Id) showed a window named Scooch $(if ($shownAt -ge 0) { "$shownAt ms after the process was made" } else { 'never within 120 s' }), UI Automation saw it $lag ms after that, in ms from the process start $(Timeline $found $Started.Id 0)"
}

function Stop-Scooch {
    $left = @(Get-Process -Name 'scooch-app', 'scoochw' -ErrorAction SilentlyContinue)
    $left | Stop-Process -Force -ErrorAction SilentlyContinue
    foreach ($process in $left) { $process.WaitForExit(15000) | Out-Null }
    $clock = [Diagnostics.Stopwatch]::StartNew()
    while ($clock.ElapsedMilliseconds -lt 15000) {
        $views = @(Get-CimInstance Win32_Process -Filter "Name = 'msedgewebview2.exe'" -ErrorAction SilentlyContinue | Where-Object { $_.CommandLine -match 'me\.sohanbhat\.scooch\.window' })
        if ($views.Count -eq 0 -and @(Get-Process -Name 'scooch-app', 'scoochw' -ErrorAction SilentlyContinue).Count -eq 0) { return [int]$clock.ElapsedMilliseconds }
        Start-Sleep -Milliseconds 200
    }
    return -1
}

function Spread($Values) {
    $sorted = @($Values | Sort-Object)
    if ($sorted.Count -eq 0) { return 'none' }
    "least $($sorted[0]), median $($sorted[[int][math]::Floor($sorted.Count / 2)]), 90th $($sorted[[int][math]::Floor($sorted.Count * 0.9)]), most $($sorted[-1]) over $($sorted.Count)"
}

function Run-Launches {
    Line ''
    Line '#### Experiment, a plain launch at varied moments after a daemon start'
    $rounds = 100
    $daemonProgram = Join-Path $folder 'scoochw.exe'
    $appProgram = Join-Path $folder 'scooch-app.exe'
    $webviewData = Join-Path $env:LOCALAPPDATA 'me.sohanbhat.scooch.window\EBWebView'
    $tally = @{ noWindow = 0; uiaLate = 0; stayed = 0; noIcon = 0; unclean = 0; byLaunch = 0; byDaemon = 0; byNone = 0; fromOther = 0 }
    $shownLaunch = @()
    $shownDaemon = @()
    $shownCold = @()
    $offsets = @()
    $lives = @()
    $lags = @()
    for ($round = 1; $round -le $rounds; $round++) {
        $clean = Stop-Scooch
        if ($clean -lt 0) { $tally.unclean++ }
        $cold = ($round % 10) -eq 0
        if ($cold) { Remove-Item -Recurse -Force $webviewData -ErrorAction SilentlyContinue }
        $delay = if ($round -le 80) { ($round - 1) * 20 } else { 2000 + ($round - 81) * 300 }
        $from = (Get-Item "$data\watch.log" -ErrorAction SilentlyContinue).Length
        if (-not $from) { $from = 0 }
        $appLog = Join-Path $script:launchFolder "app-$round.log"
        $clock = [Diagnostics.Stopwatch]::StartNew()
        $daemon = Start-Process -FilePath $daemonProgram -ArgumentList 'watch' -WindowStyle Hidden -PassThru
        while ($clock.ElapsedMilliseconds -lt $delay) { Start-Sleep -Milliseconds 5 }
        $launched = Start-Process -FilePath $appProgram -RedirectStandardError $appLog -PassThru
        $text = [Launches]::Watch($appLog, 90000, 2000)
        $found = Read-Watch $text $launched.Id
        $zero = $found.made
        $shownAt = if ($found.shown -ge 0) { $found.shown - $zero } else { -1 }
        $shower = ($found.windows | Where-Object { $_.What -eq 'shown' -and $_.Name -eq 'Scooch' } | Select-Object -First 1).Id
        $lag = if ($found.shown -ge 0) { UIA-Lag } else { -1 }
        $byDaemon = @($found.apps | Where-Object { $_.Parent -eq $daemon.Id })
        $offset = if ($byDaemon.Count -gt 0) { $byDaemon[0].Made - $zero } else { $null }
        $holders = [Seen]::Users("$data\icon.lock")
        $holder = if ($holders -eq "$($launched.Id)") { 'the launch' } elseif ($byDaemon.Count -gt 0 -and $holders -eq "$($byDaemon[0].Id)") { 'the daemon''s start' } elseif ($holders -eq 'no process') { 'no process' } else { "other $holders" }
        $life = -1
        $left = -1
        $secondSaid = ''
        if ($found.shown -ge 0) {
            $secondLog = Join-Path $script:launchFolder "second-$round.log"
            $second = Start-Process -FilePath $appProgram -RedirectStandardError $secondLog -PassThru
            if ($second.WaitForExit(30000)) { $life = [int]($second.ExitTime - $second.StartTime).TotalMilliseconds }
            Start-Sleep -Milliseconds 500
            $left = @(Get-Process -Name 'scooch-app' -ErrorAction SilentlyContinue).Count
            $secondSaid = (Get-Content -Raw $secondLog -ErrorAction SilentlyContinue) -replace "`r?`n", ' '
        }
        $iconLine = @((Read-Shared "$data\watch.log").Substring([int][math]::Min($from, (Read-Shared "$data\watch.log").Length)) -split "`r?`n" | Where-Object { $_ -match ' icon (not )?started ' }) | Select-Object -First 1
        $iconAt = if ($iconLine -match '^(\d+)\.(\d{3}) ') { [long]$Matches[1] * 1000 + [long]$Matches[2] - $zero } else { $null }
        $misses = @()
        if ($found.shown -lt 0) { $misses += 'no window'; $tally.noWindow++ }
        if ($found.shown -ge 0 -and $lag -lt 0) { $misses += 'UI Automation never saw the window'; $tally.uiaLate++ }
        if ($found.shown -ge 0 -and ($life -lt 0 -or $left -ne 1)) { $misses += 'the second launch stayed'; $tally.stayed++ }
        if ($holder -eq 'no process') { $misses += 'no icon'; $tally.noIcon++ }
        switch -Wildcard ($holder) { 'the launch' { $tally.byLaunch++ } 'the daemon*' { $tally.byDaemon++ } 'no process' { $tally.byNone++ } }
        if ($shower -and $shower -ne $launched.Id -and $byDaemon.Count -gt 0 -and $shower -ne $byDaemon[0].Id) { $tally.fromOther++ }
        if ($shownAt -ge 0) {
            if ($cold) { $shownCold += $shownAt } elseif ($holder -eq 'the launch') { $shownLaunch += $shownAt } else { $shownDaemon += $shownAt }
        }
        if ($null -ne $offset) { $offsets += $offset }
        if ($life -ge 0) { $lives += $life }
        if ($lag -ge 0) { $lags += $lag }
        $verdict = if ($misses.Count -eq 0) { 'fine' } else { "MISS $($misses -join ', ')" }
        Line "- round $round $verdict, delay $delay ms, profile $(if ($cold) { 'removed' } else { 'kept' }), the launch $($launched.Id), the daemon's start $(if ($byDaemon.Count) { "$($byDaemon[0].Id) made at $offset ms, ended $($byDaemon[0].Ended)" } else { 'not seen' }), its icon line at $iconAt ms, icon.lock held by $holder, the window showed at $shownAt ms from $shower, UI Automation $lag ms later, a second launch lived $life ms and left $left, the clean up took $clean ms"
        Add-Content -Path $script:launchLog -Value "== round $round $verdict delay $delay cold $cold launch $($launched.Id) daemon $($daemon.Id) holders $holders second said $secondSaid`n$(Timeline $found $launched.Id $daemon.Id)`n$text"
        if ($misses.Count -gt 0) {
            Line "  - $(Timeline $found $launched.Id $daemon.Id)"
            Line "  - $(App-Snapshot)"
            Line "  - the daemon's lines $(@((Read-Shared "$data\watch.log").Substring([int][math]::Min($from, (Read-Shared "$data\watch.log").Length)) -split "`r?`n" | Where-Object { $_ }) -join ' | ')"
        }
    }
    Stop-Scooch | Out-Null
    Line "- experiment, $rounds launches, no window $($tally.noWindow), UI Automation late $($tally.uiaLate), a second launch stayed $($tally.stayed), no icon after the launch $($tally.noIcon), a window from a third process $($tally.fromOther), a clean up that took over 15 s $($tally.unclean)"
    Line "- the icon lock went to the launch $($tally.byLaunch) times, to the daemon's start $($tally.byDaemon) times, to nobody $($tally.byNone) times"
    Line "- the window showed in ms from the launch, with the launch holding the icon $(Spread $shownLaunch), with the daemon's start holding it $(Spread $shownDaemon), with the profile removed $(Spread $shownCold)"
    Line "- the daemon's start was made in ms from the launch $(Spread $offsets), UI Automation saw the window in ms after it showed $(Spread $lags), a second launch lived in ms $(Spread $lives)"
}

function Save-Launch-Daemon-Log { [System.IO.File]::WriteAllText((Join-Path $Out 'daemon-watch.log'), (Read-Shared "$data\watch.log")) }

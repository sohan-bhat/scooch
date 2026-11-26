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
    [DllImport("user32.dll")]
    static extern bool EnumChildWindows(IntPtr parent, EnumProc callback, IntPtr extra);
    [DllImport("user32.dll")]
    static extern bool PostMessage(IntPtr window, uint message, IntPtr wide, IntPtr narrow);
    static bool Filled(IntPtr window) {
        bool any = false;
        EnumChildWindows(window, (child, extra) => {
            var kind = new System.Text.StringBuilder(64);
            GetClassNameW(child, kind, 64);
            any = kind.ToString().StartsWith("Chrome_");
            return !any;
        }, IntPtr.Zero);
        return any;
    }
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

    public static string Watch(string log, int cap, int after, int close) {
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
        long filled = -1, closed = -1;
        IntPtr frame = IntPtr.Zero;
        while (clock.ElapsedMilliseconds < cap && (filled < 0 || Now() < filled + after) && (closed < 0 || Now() < closed + 3000)) {
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
                    if (shown < 0 && name.ToString() == "Scooch") { shown = now; frame = window; }
                }
                return true;
            }, IntPtr.Zero);
            if (frame != IntPtr.Zero && filled < 0 && closed < 0) {
                if (Filled(frame)) {
                    filled = Now();
                    told.Add("filled " + filled);
                } else if (close >= 0 && Now() >= shown + close) {
                    PostMessage(frame, 0x0010, IntPtr.Zero, IntPtr.Zero);
                    closed = Now();
                    told.Add("closed " + closed + " blank " + !Filled(frame));
                }
            }
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
    $found = @{ apps = @(); lines = @(); windows = @(); views = @(); shown = -1; made = 0; filled = -1; closed = -1; blank = $false }
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
        } elseif ($row -match '^filled (\d+)$') {
            $found.filled = [long]$Matches[1]
        } elseif ($row -match '^closed (\d+) blank (True|False)$') {
            $found.closed = [long]$Matches[1]
            $found.blank = $Matches[2] -eq 'True'
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
    $tally = @{ noWindow = 0; uiaLate = 0; stayed = 0; noIcon = 0; unclean = 0; byLaunch = 0; byDaemon = 0; byNone = 0; quiet = 0; noisy = 0; closes = 0; blankCloses = 0; closeBad = 0; slowFrame = 0 }
    $frames = @()
    $fills = @()
    $coldFrames = @()
    $coldFills = @()
    $offsets = @()
    $lives = @()
    $lags = @()
    for ($round = 1; $round -le $rounds; $round++) {
        $clean = Stop-Scooch
        if ($clean -lt 0) { $tally.unclean++ }
        $cold = ($round % 10) -eq 0
        $quiet = ($round % 10) -eq 5
        $close = -not $quiet -and ($round % 4) -eq 2
        if ($cold) { Remove-Item -Recurse -Force $webviewData -ErrorAction SilentlyContinue }
        $delay = if ($round -le 80) { ($round - 1) * 20 } else { 2000 + ($round - 81) * 300 }
        $from = (Get-Item "$data\watch.log" -ErrorAction SilentlyContinue).Length
        if (-not $from) { $from = 0 }
        $appLog = Join-Path $script:launchFolder "app-$round.log"
        $clock = [Diagnostics.Stopwatch]::StartNew()
        $daemon = Start-Process -FilePath $daemonProgram -ArgumentList 'watch' -WindowStyle Hidden -PassThru
        if ($quiet) {
            $text = [Launches]::Watch($appLog, 6000, 0, -1)
            $found = Read-Watch $text 0
            $made = @($found.windows | Where-Object { $_.Name -eq 'Scooch' })
            $starts = @($found.apps | Where-Object { $_.Parent -eq $daemon.Id })
            if ($made.Count -eq 0 -and $starts.Count -gt 0) { $tally.quiet++ } else { $tally.noisy++ }
            Line "- round $round $(if ($made.Count -eq 0 -and $starts.Count -gt 0) { 'fine' } else { 'MISS a background start made a window or none was seen' }), a daemon start and no launch, the daemon's background starts $(@($starts | ForEach-Object Id) -join ' '), windows named Scooch it made shown or hidden $($made.Count), icon.lock held by $([Seen]::Users("$data\icon.lock"))"
            Add-Content -Path $script:launchLog -Value "== round $round quiet daemon $($daemon.Id)`n$text"
            continue
        }
        while ($clock.ElapsedMilliseconds -lt $delay) { Start-Sleep -Milliseconds 5 }
        $launched = Start-Process -FilePath $appProgram -RedirectStandardError $appLog -PassThru
        $text = [Launches]::Watch($appLog, 90000, 2000, $(if ($close) { ($round * 37) % 250 } else { -1 }))
        $found = Read-Watch $text $launched.Id
        $zero = $found.made
        $frameAt = if ($found.shown -ge 0) { $found.shown - $zero } else { -1 }
        $fillAt = if ($found.filled -ge 0) { $found.filled - $zero } else { -1 }
        $shower = ($found.windows | Where-Object { $_.What -eq 'shown' -and $_.Name -eq 'Scooch' } | Select-Object -First 1).Id
        $byDaemon = @($found.apps | Where-Object { $_.Parent -eq $daemon.Id })
        $offset = if ($byDaemon.Count -gt 0) { $byDaemon[0].Made - $zero } else { $null }
        $holders = [Seen]::Users("$data\icon.lock")
        $holder = if ($holders -eq "$($launched.Id)") { 'the launch' } elseif ($byDaemon.Count -gt 0 -and $holders -eq "$($byDaemon[0].Id)") { 'the daemon''s start' } elseif ($holders -eq 'no process') { 'no process' } else { "other $holders" }
        $misses = @()
        $lag = -1
        $afterClose = ''
        if ($close) {
            $tally.closes++
            if ($found.blank) { $tally.blankCloses++ }
            Start-Sleep -Seconds 2
            $alive = @(Get-Process -Name 'scooch-app' -ErrorAction SilentlyContinue)
            $lost = @($alive | Where-Object { "$($_.Id)" -ne $holders })
            $again = Start-Process -FilePath $appProgram -RedirectStandardError (Join-Path $script:launchFolder "again-$round.log") -PassThru
            $seen = ([Seen]::Open([Diagnostics.Stopwatch]::GetTimestamp(), 30000, 500)) -split '\|'
            Start-Sleep -Milliseconds 500
            $left = @(Get-Process -Name 'scooch-app' -ErrorAction SilentlyContinue).Count
            $afterClose = ", closed $(if ($found.blank) { 'while blank' } else { 'after it filled' }) at $($found.closed - $zero) ms, then app processes $($alive.Count) with icon.lock in $holders, others left without the lock $($lost.Count), the next launch showed its frame after $($seen[0]) ms and filled after $($seen[1]) ms leaving $left"
            if ($lost.Count -gt 0 -or $holders -eq 'no process' -or [long]$seen[1] -lt 0 -or $left -ne 1) { $misses += 'a close while blank left the app out of order'; $tally.closeBad++ }
        } else {
            if ($found.filled -ge 0) { $lag = UIA-Lag }
            if ($found.filled -lt 0) { $misses += 'no filled window'; $tally.noWindow++ }
            if ($found.filled -ge 0 -and $lag -lt 0) { $misses += 'UI Automation never saw the window'; $tally.uiaLate++ }
            if ($frameAt -lt 0 -or $frameAt -gt 3000) { $misses += 'no frame within 3 s'; $tally.slowFrame++ }
            if ($cold) { if ($frameAt -ge 0) { $coldFrames += $frameAt }; if ($fillAt -ge 0) { $coldFills += $fillAt } } else { if ($frameAt -ge 0) { $frames += $frameAt }; if ($fillAt -ge 0) { $fills += $fillAt } }
            if ($found.filled -ge 0) {
                $secondLog = Join-Path $script:launchFolder "second-$round.log"
                $second = Start-Process -FilePath $appProgram -RedirectStandardError $secondLog -PassThru
                $life = -1
                if ($second.WaitForExit(30000)) { $life = [int]($second.ExitTime - $second.StartTime).TotalMilliseconds }
                Start-Sleep -Milliseconds 500
                $left = @(Get-Process -Name 'scooch-app' -ErrorAction SilentlyContinue).Count
                if ($life -ge 0) { $lives += $life }
                if ($life -lt 0 -or $left -ne 1) { $misses += 'the second launch stayed'; $tally.stayed++ }
                $afterClose = ", a second launch lived $life ms and left $left"
            }
        }
        if ($holder -eq 'no process') { $misses += 'no icon'; $tally.noIcon++ }
        switch -Wildcard ($holder) { 'the launch' { $tally.byLaunch++ } 'the daemon*' { $tally.byDaemon++ } 'no process' { $tally.byNone++ } }
        if ($null -ne $offset) { $offsets += $offset }
        if ($lag -ge 0) { $lags += $lag }
        $verdict = if ($misses.Count -eq 0) { 'fine' } else { "MISS $($misses -join ', ')" }
        Line "- round $round $verdict, delay $delay ms, profile $(if ($cold) { 'removed' } else { 'kept' }), the launch $($launched.Id), the daemon's start $(if ($byDaemon.Count) { "$($byDaemon[0].Id) made at $offset ms" } else { 'not seen' }), icon.lock held by $holder, the frame showed at $frameAt ms and filled at $fillAt ms from $shower, UI Automation $lag ms after the fill$afterClose, the clean up took $clean ms"
        Add-Content -Path $script:launchLog -Value "== round $round $verdict delay $delay cold $cold close $close launch $($launched.Id) daemon $($daemon.Id) holders $holders`n$(Timeline $found $launched.Id $daemon.Id)`n$text"
        if ($misses.Count -gt 0) {
            Line "  - $(Timeline $found $launched.Id $daemon.Id)"
            Line "  - $(App-Snapshot)"
        }
    }
    Stop-Scooch | Out-Null
    Line "- experiment, $rounds rounds: $($rounds - 10 - $tally.closes) launches with a second launch, $($tally.closes) launches closed at their first show, 10 daemon starts with no launch"
    Line "- no filled window $($tally.noWindow), no frame within 3 s $($tally.slowFrame), UI Automation late $($tally.uiaLate), a second launch stayed $($tally.stayed), no icon after a launch $($tally.noIcon), clean ups over 15 s $($tally.unclean)"
    Line "- closes while blank $($tally.blankCloses) of $($tally.closes), closes that left the app out of order $($tally.closeBad), background starts that made no window $($tally.quiet) of 10, that made one or were not seen $($tally.noisy)"
    Line "- the icon lock went to the launch $($tally.byLaunch) times, to the daemon's start $($tally.byDaemon) times, to nobody $($tally.byNone) times"
    Line "- warm, in ms from the launch, the frame showed $(Spread $frames), the window filled $(Spread $fills)"
    Line "- with the profile removed, the frame showed $(Spread $coldFrames), the window filled $(Spread $coldFills)"
    Line "- the daemon's start was made in ms from the launch $(Spread $offsets), UI Automation saw the filled window in ms $(Spread $lags), a second launch lived in ms $(Spread $lives)"
}

function Save-Launch-Daemon-Log { [System.IO.File]::WriteAllText((Join-Path $Out 'daemon-watch.log'), (Read-Shared "$data\watch.log")) }

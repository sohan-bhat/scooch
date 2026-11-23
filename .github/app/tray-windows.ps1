Add-Type @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Runtime.InteropServices;
public static class Tray {
    [DllImport("user32.dll")]
    static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")]
    static extern void mouse_event(uint flags, int dx, int dy, uint data, UIntPtr extra);
    delegate bool EnumProc(IntPtr window, IntPtr extra);
    [DllImport("user32.dll")]
    static extern bool EnumWindows(EnumProc callback, IntPtr extra);
    [DllImport("user32.dll")]
    static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")]
    static extern bool GetWindowRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    static extern int GetWindowTextW(IntPtr window, System.Text.StringBuilder text, int count);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    static extern int GetClassNameW(IntPtr window, System.Text.StringBuilder text, int count);
    [DllImport("user32.dll")]
    static extern uint GetWindowThreadProcessId(IntPtr window, out uint pid);
    [DllImport("user32.dll")]
    static extern int GetSystemMetrics(int index);
    public struct Rect { public int Left; public int Top; public int Right; public int Bottom; }
    public static string Covers() {
        var told = new List<string>();
        long screen = (long)GetSystemMetrics(0) * GetSystemMetrics(1);
        EnumWindows((window, extra) => {
            if (!IsWindowVisible(window)) return true;
            Rect rect;
            if (!GetWindowRect(window, out rect)) return true;
            long area = (long)(rect.Right - rect.Left) * (rect.Bottom - rect.Top);
            if (area * 10 < screen * 6) return true;
            var name = new System.Text.StringBuilder(256);
            var kind = new System.Text.StringBuilder(256);
            GetWindowTextW(window, name, 256);
            GetClassNameW(window, kind, 256);
            string cls = kind.ToString();
            if (cls == "Progman" || cls == "WorkerW" || cls == "Shell_TrayWnd") return true;
            uint pid;
            GetWindowThreadProcessId(window, out pid);
            told.Add(pid + "|" + cls + "|" + name);
            return true;
        }, IntPtr.Zero);
        return string.Join(";", told);
    }
    [DllImport("user32.dll", SetLastError = true)]
    static extern IntPtr OpenInputDesktop(uint flags, bool inherit, uint access);
    [DllImport("user32.dll", SetLastError = true)]
    static extern IntPtr GetThreadDesktop(uint thread);
    [DllImport("kernel32.dll")]
    static extern uint GetCurrentThreadId();
    [DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    static extern bool GetUserObjectInformationW(IntPtr handle, int index, System.Text.StringBuilder info, int length, out int needed);
    [DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    static extern IntPtr OpenDesktopW(string name, uint flags, bool inherit, uint access);
    [DllImport("user32.dll", SetLastError = true)]
    static extern bool SwitchDesktop(IntPtr desktop);
    [DllImport("user32.dll")]
    static extern bool CloseDesktop(IntPtr desktop);
    [DllImport("user32.dll")]
    static extern bool GetCursorPos(out Spot spot);
    [DllImport("user32.dll")]
    static extern IntPtr WindowFromPoint(Spot spot);
    public struct Spot { public int X; public int Y; }
    static string NameOf(IntPtr desktop) {
        var name = new System.Text.StringBuilder(256);
        int needed;
        return GetUserObjectInformationW(desktop, 2, name, name.Capacity * 2, out needed) ? name.ToString() : "unnamed";
    }
    public static string OwnDesktop() { return NameOf(GetThreadDesktop(GetCurrentThreadId())); }
    public static string InputDesktop() {
        IntPtr desktop = OpenInputDesktop(0, false, 0x0001);
        if (desktop == IntPtr.Zero) return "unreadable, error " + Marshal.GetLastWin32Error();
        try { return NameOf(desktop); } finally { CloseDesktop(desktop); }
    }
    public static string SwitchTo(string name) {
        IntPtr desktop = OpenDesktopW(name, 0, false, 0x0100);
        if (desktop == IntPtr.Zero) return "could not open it, error " + Marshal.GetLastWin32Error();
        bool done = SwitchDesktop(desktop);
        int error = Marshal.GetLastWin32Error();
        CloseDesktop(desktop);
        return done ? "worked" : "failed, error " + error;
    }
    public static string Cursor() {
        Spot spot;
        return GetCursorPos(out spot) ? spot.X + " " + spot.Y : "unknown";
    }
    public static string Under(int x, int y) {
        var spot = new Spot();
        spot.X = x;
        spot.Y = y;
        IntPtr window = WindowFromPoint(spot);
        if (window == IntPtr.Zero) return "none";
        var kind = new System.Text.StringBuilder(256);
        GetClassNameW(window, kind, 256);
        uint pid;
        GetWindowThreadProcessId(window, out pid);
        return kind + " of process " + pid;
    }
    public static void Move(int x, int y) {
        SetCursorPos(x, y);
    }
    public static void Press(int x, int y, bool right) {
        SetCursorPos(x, y);
        System.Threading.Thread.Sleep(200);
        mouse_event(right ? 0x0008u : 0x0002u, 0, 0, 0, UIntPtr.Zero);
        System.Threading.Thread.Sleep(80);
        mouse_event(right ? 0x0010u : 0x0004u, 0, 0, 0, UIntPtr.Zero);
    }
    [DllImport("ntdll.dll")]
    static extern int NtQuerySystemInformation(int kind, IntPtr buffer, int length, out int needed);
    public static Dictionary<long, long> Snap(int pid) {
        var told = new Dictionary<long, long>();
        int length = 1 << 20;
        while (true) {
            IntPtr buffer = Marshal.AllocHGlobal(length);
            try {
                int needed;
                int status = NtQuerySystemInformation(5, buffer, length, out needed);
                if (status == unchecked((int)0xC0000004)) { length *= 2; continue; }
                if (status != 0) return told;
                long offset = 0;
                while (true) {
                    IntPtr entry = new IntPtr(buffer.ToInt64() + offset);
                    int next = Marshal.ReadInt32(entry, 0);
                    int threads = Marshal.ReadInt32(entry, 4);
                    long id = Marshal.ReadIntPtr(entry, 0x50).ToInt64();
                    if (id == pid) {
                        for (int index = 0; index < threads; index++) {
                            int at = 0x100 + index * 0x50;
                            long thread = Marshal.ReadIntPtr(entry, at + 0x30).ToInt64();
                            told[thread] = (uint)Marshal.ReadInt32(entry, at + 0x40);
                        }
                        return told;
                    }
                    if (next == 0) return told;
                    offset += next;
                }
            } finally {
                Marshal.FreeHGlobal(buffer);
            }
        }
    }
    public static long Woke(Dictionary<long, long> before, Dictionary<long, long> after) {
        long total = 0;
        foreach (var pair in after) {
            long was;
            total += before.TryGetValue(pair.Key, out was) ? pair.Value - was : pair.Value;
        }
        return total;
    }
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct Entry {
        public uint Size; public uint Usage; public uint Id; public IntPtr Heap; public uint Module; public uint Threads; public uint Parent; public int Base; public uint Flags;
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
    public static string Starts(string name, int milliseconds, int[] known) {
        var seen = new Dictionary<uint, uint>();
        var watch = Stopwatch.StartNew();
        while (watch.ElapsedMilliseconds < milliseconds) {
            IntPtr snapshot = CreateToolhelp32Snapshot(2, 0);
            var entry = new Entry();
            entry.Size = (uint)Marshal.SizeOf(typeof(Entry));
            if (Process32FirstW(snapshot, ref entry)) {
                do {
                    if (string.Equals(entry.Name, name, StringComparison.OrdinalIgnoreCase) && Array.IndexOf(known, (int)entry.Id) < 0) seen[entry.Id] = entry.Parent;
                } while (Process32NextW(snapshot, ref entry));
            }
            CloseHandle(snapshot);
            System.Threading.Thread.Sleep(15);
        }
        var told = new List<string>();
        foreach (var pair in seen) told.Add(pair.Key + " " + pair.Value);
        return string.Join(";", told);
    }
    [DllImport("kernel32.dll")]
    static extern IntPtr OpenProcess(uint access, bool inherit, int pid);
    [DllImport("kernel32.dll")]
    static extern bool GetExitCodeProcess(IntPtr process, out uint code);
    public static string Object(int pid) {
        IntPtr process = OpenProcess(0x1000, false, pid);
        if (process == IntPtr.Zero) return "gone";
        uint code;
        GetExitCodeProcess(process, out code);
        CloseHandle(process);
        return code == 259 ? "running" : "kept after it ended";
    }
}
'@

function Icon-Processes { @(Get-Process -Name 'scooch-app' -ErrorAction SilentlyContinue) }
function Icon-Count { (Icon-Processes).Count }
function Icon-Pid { (Icon-Processes | Select-Object -First 1).Id }

function Tray-Button {
    $root = [System.Windows.Automation.AutomationElement]::RootElement
    $shell = @(Get-Process -Name explorer -ErrorAction SilentlyContinue | ForEach-Object Id)
    $buttons = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::Button)
    foreach ($top in $root.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)) {
        if ($shell -notcontains $top.Current.ProcessId) { continue }
        foreach ($button in $top.FindAll([System.Windows.Automation.TreeScope]::Descendants, $buttons)) {
            if ($button.Current.Name -eq 'Scooch' -and $button.Current.BoundingRectangle.Width -gt 0) { return $button }
        }
    }
    return $null
}

function Promote-Icon {
    $done = @()
    foreach ($key in @(Get-ChildItem 'HKCU:\Control Panel\NotifyIconSettings' -ErrorAction SilentlyContinue)) {
        $path = (Get-ItemProperty $key.PSPath -ErrorAction SilentlyContinue).ExecutablePath
        if ($path -like '*scooch-app.exe') {
            Set-ItemProperty $key.PSPath -Name IsPromoted -Value 1 -Type DWord
            $done += $key.PSChildName
        }
    }
    return $done
}

function Find-Icon {
    $found = Tray-Button
    if ($found) { return $found }
    $promoted = Promote-Icon
    if ($promoted.Count -gt 0) {
        Line "- the icon sat among the hidden ones, the probe set it to show on the taskbar as Settings does, key $($promoted -join ' ')"
        Start-Sleep -Seconds 3
        $found = Tray-Button
        if ($found) { return $found }
    }
    $root = [System.Windows.Automation.AutomationElement]::RootElement
    $chevron = $null
    foreach ($button in $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::Button)))) {
        if ($button.Current.Name -match 'hidden icons') { $chevron = $button; break }
    }
    if ($chevron) {
        $invoke = $null
        if ($chevron.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$invoke)) { $invoke.Invoke() }
        Start-Sleep -Seconds 2
        Line "- opened the hidden icons through $($chevron.Current.Name)"
        return Tray-Button
    }
    return $null
}

function Icon-Center {
    $icon = Find-Icon
    if (-not $icon) { return $null }
    $box = $icon.Current.BoundingRectangle
    return @([int]($box.X + $box.Width / 2), [int]($box.Y + $box.Height / 2), $icon.Current.Name)
}

function Tray-Menu {
    $root = [System.Windows.Automation.AutomationElement]::RootElement
    $owner = @(Icon-Processes | ForEach-Object Id)
    foreach ($top in $root.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)) {
        if ($owner -contains $top.Current.ProcessId -and $top.Current.ControlType -eq [System.Windows.Automation.ControlType]::Menu) { return $top }
    }
    return $null
}

function Menu-Items {
    $menu = Tray-Menu
    if (-not $menu) { return @() }
    $items = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::MenuItem)
    return @($menu.FindAll([System.Windows.Automation.TreeScope]::Descendants, $items))
}

function Menu-Reads {
    @(Menu-Items | Where-Object { $_.Current.Name } | ForEach-Object {
        $toggle = $null
        $mark = ''
        if ($_.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern, [ref]$toggle)) { $mark = " $($toggle.Current.ToggleState)" }
        "$($_.Current.Name)$mark"
    }) -join '|'
}

function Open-Menu {
    $spot = Icon-Center
    if (-not $spot) { return 'no icon found in the notification area' }
    $before = Tray-Lines 'Right Up'
    [Tray]::Press($spot[0], $spot[1], $true)
    $landed = if (Landed 'Right Up' $before) { 'landed on the icon, the app logged it, and' } else { 'was not logged by the app and' }
    $shown = Wait-For 6 { [bool](Tray-Menu) }
    if ($shown -ge 0) { return "a right press on the icon $($spot[2]) at $($spot[0]) $($spot[1]) $landed opened the menu" }
    return "a right press on the icon $($spot[2]) $landed opened no menu"
}

function Press-Item([string]$Label) {
    $opened = Open-Menu
    Line "- $opened"
    Line "- the menu reads $(Menu-Reads)"
    foreach ($item in Menu-Items) {
        if ($item.Current.Name -eq $Label) {
            $invoke = $null
            if ($item.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$invoke)) {
                $invoke.Invoke()
                Start-Sleep -Seconds 2
                return "pressed $Label in the menu through UI Automation"
            }
        }
    }
    return "found no $Label in the menu"
}

function Quiet-Minute([string]$Which) {
    [Tray]::Move(300, 200)
    Start-Sleep -Seconds 3
    $process = Icon-Processes | Select-Object -First 1
    $before = [Tray]::Snap($process.Id)
    $cpu = $process.TotalProcessorTime.TotalMilliseconds
    Start-Sleep -Seconds 60
    $process.Refresh()
    $script:quietSwitches = [Tray]::Woke($before, [Tray]::Snap($process.Id))
    $used = [int]($process.TotalProcessorTime.TotalMilliseconds - $cpu)
    Line "- over a quiet minute, $Which, the icon process $($process.Id) woke its threads $($script:quietSwitches) times over $($process.Threads.Count) threads, used $used ms of CPU, and holds $([math]::Round($process.PrivateMemorySize64 / 1MB, 1)) MB private with a working set of $([math]::Round($process.WorkingSet64 / 1MB, 1)) MB"
}

function Clear-Covers {
    $done = @()
    foreach ($cover in @([Tray]::Covers() -split ';' | Where-Object { $_ })) {
        $parts = $cover -split '\|', 3
        $process = Get-Process -Id ([int]$parts[0]) -ErrorAction SilentlyContinue
        $name = if ($process) { $process.ProcessName } else { 'gone' }
        $done += "$name $($parts[1]) '$($parts[2])'"
        if ($name -match 'WWAHost|OOBE|CloudExperience|msoobe') {
            Stop-Process -Id ([int]$parts[0]) -Force -ErrorAction SilentlyContinue
            $done[-1] += ' ended'
        }
    }
    return $done
}

function Clear-Privacy {
    $labels = 'Accept', 'Next', 'Skip', 'Not now', 'Skip for now', 'Continue', 'Close'
    $pressed = @()
    for ($round = 0; $round -lt 12; $round++) {
        $setup = @(Setup-Windows)
        if ($setup.Count -eq 0) { break }
        $done = $false
        foreach ($window in $setup) {
            foreach ($label in $labels) {
                $byName = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, $label)
                $button = $window.FindAll([System.Windows.Automation.TreeScope]::Descendants, $byName) | Where-Object { $_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Button } | Select-Object -First 1
                $invoke = $null
                if ($button -and $button.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$invoke)) {
                    $invoke.Invoke()
                    $pressed += $label
                    $done = $true
                    break
                }
            }
            if ($done) { break }
        }
        if (-not $done) { break }
        Start-Sleep -Seconds 3
    }
    return $pressed
}

function Close-Window {
    $window = Find-Window
    if ($window) { $window.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).Close() }
    Wait-For 15 { -not (Find-Window) } | Out-Null
    Start-Sleep -Seconds 2
}

$trayLog = Join-Path $Out 'tray.log'
function Tray-Lines([string]$Kind) { @((Read-Shared $trayLog) -split "`n" | Where-Object { $_ -match "scooch-app tray $Kind" }).Count }
function Landed([string]$Kind, [int]$Before) { (Wait-For 4 { (Tray-Lines $Kind) -gt $Before }) -ge 0 }

Line ''
Line '### The notification area icon'
$own = [Tray]::OwnDesktop()
$screenDesk = [Tray]::InputDesktop()
Line "- the probe runs on the desktop $own, the screen shows the input desktop $screenDesk"
if ($screenDesk -ne $own) {
    Line "- switching the screen to $own $([Tray]::SwitchTo($own)), the input desktop is now $([Tray]::InputDesktop())"
}
$privacy = Clear-Privacy
if ($privacy.Count -gt 0) {
    Line "- a Windows setup screen covered the taskbar, the probe pressed $($privacy -join ', ') in it, setup windows left $(@(Setup-Windows).Count)"
}
$covers = Clear-Covers
if ($covers.Count -gt 0) {
    Line "- windows that cover most of the screen $($covers -join '; ')"
}
Icon-Processes | Stop-Process -Force -ErrorAction SilentlyContinue
Wait-For 10 { (Icon-Count) -eq 0 } | Out-Null
Start-Process -FilePath (Join-Path $folder 'scooch-app.exe') -RedirectStandardError $trayLog
Wait-For 40 { [bool](Find-Window) } | Out-Null
Start-Sleep -Seconds 4
Close-Window
$spot = Icon-Center
Line "- started the app again so its presses log to tray.log, closed its window, window $([bool](Find-Window)), app processes $(Icon-Count), the icon $(if ($spot) { "$($spot[2]) at $($spot[0]) $($spot[1])" } else { 'not found' })"
Expect "closing the window while Scooch is on leaves the app process" ((Icon-Count) -eq 1)
Expect "closing the window removes the window" (-not (Find-Window))
Expect "the icon sits in the notification area after the window closed" ([bool]$spot)
Screen-Shot 'notification-area'
Quiet-Minute 'after the window closed'

Line ''
Line '#### A left press opens the window, a right press opens the menu'
$leftBefore = Tray-Lines 'Left Up'
[Tray]::Press($spot[0], $spot[1], $false)
$script:footing = Landed 'Left Up' $leftBefore
if ($script:footing) {
    $shown = Wait-For 20 { [bool](Find-Window) }
    Line "- the left press landed on the icon, the app logged it, and the window showed after $shown s"
    Expect "a left press on the icon opens the window" ($shown -ge 0)
    Close-Window
    $opened = Open-Menu
    Line "- $opened"
    $read = Menu-Reads
    Line "- the menu reads $read"
    Expect "the menu reads Open Scooch, On and Quit Scooch" ($read -match '^Open Scooch\|On( On| Off)?\|Quit Scooch$')
    if ($read -match '\|On (On|Off)\|') {
        Expect "On is checked while Scooch is on" ($read -match '\|On On\|')
    } else {
        Limit "the menu item On shows no check state to UI Automation here, so its mark is read only on macOS and Linux"
    }
    Line "- $(Press-Item 'Open Scooch')"
    $shown = Wait-For 20 { [bool](Find-Window) }
    Line "- Open Scooch showed the window after $shown s"
    Expect "Open Scooch in the menu opens the window" ($shown -ge 0)

    Line ''
    Line '#### On in the menu turns Scooch off and on with the window open'
    Line "- $(Press-Item 'On')"
    $off = Wait-For 30 { -not (State-On) }
    Start-Sleep -Seconds 3
    Line "- after On, off after $off s, wallpaper $([Desk]::Wallpaper()), app processes $(Icon-Count), the icon $([bool](Tray-Button))"
    Expect "On in the menu turned Scooch off" ($off -ge 0)
    Expect "off from the menu put the photo back" ([Desk]::Wallpaper() -ieq $photo)
    Expect "with the window open the icon stays while Scooch is off" ([bool](Tray-Button))
    Line "- $(Press-Item 'On')"
    $again = Wait-For 30 { State-On }
    Wait-For 40 { (Read-Shared "$data\state.json") -match '"fingerprint": "' } | Out-Null
    Start-Sleep -Seconds 3
    Line "- after On again, on after $again s, wallpaper $([Desk]::Wallpaper())"
    Expect "On in the menu turned Scooch on again" ($again -ge 0)
    Expect "on from the menu drew the desktop" ([Desk]::Wallpaper() -ine $photo)
} else {
    Screen-Shot 'press-not-landed'
    Line "- the left press at $($spot[0]) $($spot[1]) never reached the app, which logged no tray press, the cursor is at $([Tray]::Cursor()), the window under it is $([Tray]::Under($spot[0], $spot[1])), the input desktop is $([Tray]::InputDesktop())"
    Limit "on this runner no mouse press reaches the taskbar, so the presses on the icon and its menu are not judged here. The same presses are judged on the other Windows runner"
    Line "- the window $(if (Find-Window) { 'opened anyway' } else { 'stayed closed' })"
}

Line ''
Line '#### Opening Scooch while only the icon runs'
Close-Window
$before = Icon-Pid
Start-Process -FilePath (Join-Path $folder 'scooch-app.exe')
$shown = Wait-For 20 { [bool](Find-Window) }
Start-Sleep -Seconds 2
Line "- starting scooch-app.exe showed the window after $shown s, app processes $(Icon-Count), the same one $((Icon-Pid) -eq $before)"
Expect "opening Scooch while the icon runs shows the window" ($shown -ge 0)
Expect "opening Scooch while the icon runs keeps one app process" ((Icon-Count) -eq 1)
Start-Process -FilePath (Join-Path $folder 'scooch-app.exe') -ArgumentList '--background'
Start-Sleep -Seconds 6
Line "- a start with the hidden argument while the app runs left $(Icon-Count) app process"
Expect "a background start while the app runs leaves one app process" ((Icon-Count) -eq 1)
Close-Window

Line ''
Line '#### Quit Scooch'
if ($script:footing) {
    Line "- $(Press-Item 'Quit Scooch')"
} else {
    $command = Start-Process -FilePath (Join-Path $folder 'scoochw.exe') -ArgumentList 'off' -WindowStyle Hidden -PassThru
    $command.WaitForExit(60000) | Out-Null
    Line "- with no press reaching the icon, scooch off turned Scooch off in its place"
}
$gone = Wait-For 30 { (Icon-Count) -eq 0 }
$off = Wait-For 30 { -not (State-On) }
Start-Sleep -Seconds 3
Line "- after Quit, app processes $(Icon-Count) after $gone s, on $(State-On), wallpaper $([Desk]::Wallpaper()), login entry '$(Run-Value)', daemons $(Daemons)"
if ($script:footing) {
    Expect "Quit Scooch turned Scooch off" (-not (State-On))
    Expect "Quit Scooch put the photo back" ([Desk]::Wallpaper() -ieq $photo)
    Expect "Quit Scooch removed the login entry" (-not (Run-Value))
    Expect "Quit Scooch ended the app process" ((Icon-Count) -eq 0)
} else {
    Expect "turning Scooch off with no window open ended the icon" ((Icon-Count) -eq 0)
}

Line ''
Line '#### A background start with Scooch off'
$hidden = Start-Process -FilePath (Join-Path $folder 'scooch-app.exe') -ArgumentList '--background' -PassThru
$ended = $hidden.WaitForExit(10000)
Line "- the hidden start with Scooch off ended $ended, app processes $(Icon-Count)"
Expect "a background start with Scooch off ends at once" $ended
Expect "a background start with Scooch off leaves no app process" ((Icon-Count) -eq 0)

Line ''
Line '#### Login with Scooch on'
Start-Process -FilePath (Join-Path $folder 'scoochw.exe') -WindowStyle Hidden
Wait-For 30 { State-On } | Out-Null
$up = Wait-For 30 { (Icon-Count) -ge 1 }
Line "- scoochw turned Scooch on, the daemon started the icon after $up s"
Expect "a daemon started from the login entry starts the icon" ($up -ge 0)
Get-Process -Name 'scooch-app', 'scoochw' -ErrorAction SilentlyContinue | Stop-Process -Force
Wait-For 15 { (Icon-Count) -eq 0 -and (Daemons) -eq 0 } | Out-Null
$entry = Run-Value
Line "- logged out in effect, no daemon $((Daemons) -eq 0), no app $((Icon-Count) -eq 0), the login entry runs $entry"
$program = if ($entry -match '^"([^"]+)"\s*(.*)$') { $Matches[1] } else { ($entry -split ' ')[0] }
$rest = if ($entry -match '^"([^"]+)"\s*(.*)$') { $Matches[2] } else { ($entry -split ' ', 2)[1] }
Start-Process -FilePath $program -ArgumentList $rest -WindowStyle Hidden
$up = Wait-For 30 { (Icon-Count) -ge 1 }
Start-Sleep -Seconds 4
$icon = Icon-Pid
$watcher = (Get-Process -Name scoochw -ErrorAction SilentlyContinue | Select-Object -First 1).Id
$parent = (Get-CimInstance Win32_Process -Filter "ProcessId = $icon" -ErrorAction SilentlyContinue).ParentProcessId
Line "- the icon process $icon came up $up s after the login entry ran, made by $parent, the daemon is $watcher"
$spot = Icon-Center
Expect "after a login with Scooch on the icon is there" ([bool]$spot)
Expect "after a login with Scooch on no window opens" (-not (Find-Window))
Quiet-Minute 'started at the login with no window ever'
Expect "the icon started at the login woke its threads at most 6 times in a quiet minute" ($script:quietSwitches -ge 0 -and $script:quietSwitches -le 6)
Get-Process -Name scoochw -ErrorAction SilentlyContinue | Stop-Process -Force
Wait-For 15 { (Daemons) -eq 0 } | Out-Null
Start-Sleep -Seconds 3
Line "- stopped the daemon, the icon process $icon $(if (Get-Process -Id $icon -ErrorAction SilentlyContinue) { 'still runs' } else { 'ended' })"
Expect "the icon does not die with the daemon" ([bool](Get-Process -Id $icon -ErrorAction SilentlyContinue))
$known = @(Icon-Processes | ForEach-Object Id)
Start-Process -FilePath $program -ArgumentList $rest -WindowStyle Hidden
$starts = [Tray]::Starts('scooch-app.exe', 8000, [int[]]$known)
$watcher = (Get-Process -Name scoochw -ErrorAction SilentlyContinue | Select-Object -First 1).Id
Line "- the login entry started the daemon $watcher again while the icon ran, new app processes as id and maker $starts, app processes now $(Icon-Count)"
$maker = if ($starts -match '^\d+ (\d+)$') { [int]$Matches[1] } else { 0 }
Expect "a daemon that starts again while the icon runs leaves one app process" ((Icon-Count) -eq 1)
Expect "the daemon started one short lived app process that the first one took over" ($maker -eq $watcher)
Start-Sleep -Seconds 60
$short = if ($starts -match '^(\d+) ') { [int]$Matches[1] } else { 0 }
$kept = if ($short) { [Tray]::Object($short) } else { 'unknown' }
Line "- a minute later the short lived app process $short is $kept, the daemon $watcher $(if (Get-Process -Id $watcher -ErrorAction SilentlyContinue) { 'runs' } else { 'is gone' })"
Expect "a minute later no ended child of the daemon is kept by a handle" ($kept -eq 'gone')
Wait-For 40 { (Read-Shared "$data\state.json") -match '"fingerprint": "' } | Out-Null
Line "- Scooch is on with only the icon for the rest of the probe, app processes $(Icon-Count)"

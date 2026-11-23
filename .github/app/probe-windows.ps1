param(
    [Parameter(Mandatory = $true)][string]$Built,
    [Parameter(Mandatory = $true)][string]$Out,
    [string]$Programs = ''
)

$ErrorActionPreference = 'Continue'
New-Item -ItemType Directory -Force $Out | Out-Null
$report = Join-Path $Out 'report.md'
Set-Content -Path $report -Value ''
$folder = Join-Path $env:LOCALAPPDATA 'Programs\Scooch'
$data = Join-Path $env:LOCALAPPDATA 'Scooch'
$photo = Join-Path (Resolve-Path 'assets/photos').Path 'golden-tree.jpg'
foreach ($shared in 'report', 'folder', 'data', 'photo') { (Get-Variable -Name $shared).Options = 'ReadOnly' }

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Drawing, System.Windows.Forms
Add-Type @'
using System;
using System.Runtime.InteropServices;
[ComImport, Guid("B92B56A9-8B55-4E14-9A89-0199BBB6F93B"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IDesktopWallpaper {
    void SetWallpaper([MarshalAs(UnmanagedType.LPWStr)] string monitor, [MarshalAs(UnmanagedType.LPWStr)] string wallpaper);
    [return: MarshalAs(UnmanagedType.LPWStr)] string GetWallpaper([MarshalAs(UnmanagedType.LPWStr)] string monitor);
    [return: MarshalAs(UnmanagedType.LPWStr)] string GetMonitorDevicePathAt(uint index);
    uint GetMonitorDevicePathCount();
}
[ComImport, Guid("C2CF3110-460E-4fc1-B9D0-8A1C0C9CC4BD")]
public class DesktopWallpaperClass {}
public struct Spot { public int X; public int Y; }
public struct Box { public int Left; public int Top; public int Right; public int Bottom; }
public static class Desk {
    [DllImport("user32.dll")]
    public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")]
    public static extern bool GetClientRect(IntPtr window, out Box box);
    [DllImport("user32.dll")]
    public static extern bool ClientToScreen(IntPtr window, ref Spot spot);
    public static int[] Client(IntPtr window) {
        Box box;
        if (!GetClientRect(window, out box)) return null;
        Spot corner = new Spot();
        if (!ClientToScreen(window, ref corner)) return null;
        return new int[] { corner.X, corner.Y, corner.X + box.Right, corner.Y + box.Bottom };
    }
    [DllImport("user32.dll", EntryPoint = "SendMessageW")]
    public static extern IntPtr SendMessage(IntPtr window, uint message, IntPtr wide, IntPtr narrow);
    public static void Type(IntPtr box, string text) {
        SendMessage(box, 0x00B1, IntPtr.Zero, new IntPtr(-1));
        SendMessage(box, 0x0102, new IntPtr(8), IntPtr.Zero);
        foreach (char c in text) SendMessage(box, 0x0102, new IntPtr(c), IntPtr.Zero);
    }
    [DllImport("user32.dll")]
    public static extern bool PostMessage(IntPtr window, uint message, IntPtr wide, IntPtr narrow);
    public static void Enter(IntPtr box) {
        PostMessage(box, 0x0100, new IntPtr(0x0D), IntPtr.Zero);
        PostMessage(box, 0x0101, new IntPtr(0x0D), IntPtr.Zero);
    }
    [DllImport("user32.dll")]
    public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")]
    public static extern void mouse_event(uint flags, int dx, int dy, uint data, UIntPtr extra);
    public static void Click(int x, int y) {
        SetCursorPos(x, y);
        System.Threading.Thread.Sleep(150);
        mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero);
        System.Threading.Thread.Sleep(80);
        mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero);
    }
    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr window);
    [DllImport("user32.dll")]
    public static extern bool PrintWindow(IntPtr window, IntPtr hdc, uint flags);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr SendMessageTimeout(IntPtr window, uint message, UIntPtr wide, string text, uint flags, uint timeout, out UIntPtr result);
    [DllImport("user32.dll")]
    public static extern bool SetWindowPos(IntPtr window, IntPtr after, int x, int y, int width, int height, uint flags);
    [DllImport("user32.dll")]
    public static extern IntPtr GetDesktopWindow();
    [DllImport("user32.dll")]
    public static extern bool ShowWindow(IntPtr window, int command);
    public static void Announce(string what) {
        UIntPtr result;
        SendMessageTimeout(new IntPtr(0xffff), 0x1A, UIntPtr.Zero, what, 2, 5000, out result);
    }
    static IDesktopWallpaper Shell() { return (IDesktopWallpaper)new DesktopWallpaperClass(); }
    public static string Wallpaper() {
        var shell = Shell();
        return shell.GetWallpaper(shell.GetMonitorDevicePathAt(0));
    }
    [DllImport("user32.dll")]
    static extern bool SystemParametersInfo(int action, int param, ref bool value, int flags);
    [DllImport("user32.dll", EntryPoint = "SystemParametersInfo")]
    static extern bool SystemParametersFlag(int action, int param, IntPtr value, int flags);
    public static bool Animations() {
        bool on = true;
        SystemParametersInfo(0x1042, 0, ref on, 0);
        return on;
    }
    public static void SetAnimations(bool on) {
        SystemParametersFlag(0x1043, 0, new IntPtr(on ? 1 : 0), 3);
    }
    public static void Set(string path) {
        Shell().SetWallpaper(null, path);
    }
}
'@
[Desk]::SetProcessDPIAware() | Out-Null

function Line([string]$Text) { Add-Content -Path $report -Value $Text; Write-Host $Text }
. (Join-Path $PSScriptRoot 'expect.ps1')
function Light-Shot([string]$Name) { return (Shot-Brightness $Name) -gt 0.6 }
function Dark-Shot([string]$Name) { return (Shot-Brightness $Name) -lt 0.3 }
function Shot-Brightness([string]$Name) {
    $path = Join-Path $Out "$Name.png"
    if (-not (Test-Path $path)) { return 0.5 }
    $image = [System.Drawing.Bitmap]::FromFile($path)
    try { return $image.GetPixel([int]($image.Width / 2), 45).GetBrightness() } finally { $image.Dispose() }
}
function Limit([string]$Text) { Line "- limit: $Text" }

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

function Shot([string]$Name) {
    $window = Find-Window
    if (-not $window) { Line "- no window for $Name"; return }
    $box = $window.Current.BoundingRectangle
    $bitmap = New-Object System.Drawing.Bitmap([int]$box.Width, [int]$box.Height)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $hdc = $graphics.GetHdc()
    $printed = [Desk]::PrintWindow([IntPtr]$window.Current.NativeWindowHandle, $hdc, 2)
    $graphics.ReleaseHdc($hdc)
    if (-not $printed) { $graphics.CopyFromScreen([int]$box.X, [int]$box.Y, 0, 0, $bitmap.Size) }
    $bitmap.Save((Join-Path $Out "$Name.png"), [System.Drawing.Imaging.ImageFormat]::Png)
    $graphics.Dispose()
    $bitmap.Dispose()
    Line "- window shot $Name.png at $([int]$box.Width)x$([int]$box.Height)"
}

function Press([string]$Label, [int]$Tabs) {
    $byName = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, $Label)
    for ($attempt = 0; $attempt -lt 10; $attempt++) {
        $window = Find-Window
        if ($window) {
            foreach ($found in $window.FindAll([System.Windows.Automation.TreeScope]::Descendants, $byName)) {
                $invoke = $null
                if ($found.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$invoke)) {
                    $invoke.Invoke()
                    return "pressed $Label through UI Automation"
                }
                $toggle = $null
                if ($found.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern, [ref]$toggle)) {
                    $toggle.Toggle()
                    return "toggled $Label through UI Automation"
                }
            }
        }
        Start-Sleep -Milliseconds 700
    }
    $window = Find-Window
    if (-not $window) { return "no window to press $Label in" }
    [Desk]::SetForegroundWindow([IntPtr]$window.Current.NativeWindowHandle) | Out-Null
    Start-Sleep -Milliseconds 500
    $shell = New-Object -ComObject WScript.Shell
    $shell.SendKeys(('{TAB}' * $Tabs) + '~')
    return "pressed $Label with $Tabs Tabs and Enter, since UI Automation found no button named $Label"
}

function Read-Shared([string]$Path) {
    try {
        $stream = [System.IO.File]::Open($Path, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]'ReadWrite, Delete')
        try { return (New-Object System.IO.StreamReader($stream)).ReadToEnd() } finally { $stream.Dispose() }
    } catch { return '' }
}
function Shared-Sum([string]$Path) {
    $text = Read-Shared $Path
    return [System.BitConverter]::ToString([System.Security.Cryptography.MD5]::HashData([System.Text.Encoding]::UTF8.GetBytes($text)))
}
function State-On { (Test-Path "$data\state.json") -and ((Read-Shared "$data\state.json") -match '"on": true') }
function Run-Value { (Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name Scooch -ErrorAction SilentlyContinue).Scooch }
function Daemons { @(Get-Process -Name scoochw -ErrorAction SilentlyContinue).Count }

function Screen-Shot([string]$Name) {
    $bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bitmap = New-Object System.Drawing.Bitmap($bounds.Width, $bounds.Height)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.CopyFromScreen($bounds.X, $bounds.Y, 0, 0, $bitmap.Size)
    $bitmap.Save((Join-Path $Out "$Name.png"), [System.Drawing.Imaging.ImageFormat]::Png)
    $graphics.Dispose()
    $bitmap.Dispose()
}

function Top-Windows {
    $root = [System.Windows.Automation.AutomationElement]::RootElement
    @($root.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition) | ForEach-Object {
        "$($_.Current.Name) from $((Get-Process -Id $_.Current.ProcessId -ErrorAction SilentlyContinue).ProcessName)"
    }) -join '; '
}

function Setup-Windows {
    $root = [System.Windows.Automation.AutomationElement]::RootElement
    foreach ($window in $root.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)) {
        $name = $window.Current.Name
        $process = (Get-Process -Id $window.Current.ProcessId -ErrorAction SilentlyContinue).ProcessName
        if ($name -eq 'Microsoft account' -or $process -in 'WWAHost', 'UserOOBEBroker', 'CloudExperienceHostBroker', 'msoobe') { $window }
    }
}

function Clear-Setup {
    $done = @()
    if (@(Setup-Windows).Count -eq 0) { return $done }
    $engagement = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\UserProfileEngagement'
    if (-not (Test-Path $engagement)) { New-Item $engagement | Out-Null }
    Set-ItemProperty $engagement -Name ScoobeSystemSettingEnabled -Value 0 -Type DWord
    $policy = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\OOBE'
    if (-not (Test-Path $policy)) { New-Item $policy -ErrorAction SilentlyContinue | Out-Null }
    Set-ItemProperty $policy -Name DisablePrivacyExperience -Value 1 -Type DWord -ErrorAction SilentlyContinue
    $done += 'turned the setup screen off in the registry'
    Stop-Process -Name explorer -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 4
    if (Get-Process -Name explorer -ErrorAction SilentlyContinue) {
        $done += 'Explorer came back on its own'
    } else {
        Start-Process explorer.exe
        $done += 'started Explorer again'
    }
    $desk = Wait-For 30 { @(Get-Process -Name explorer -ErrorAction SilentlyContinue).Count -gt 0 -and (Top-Windows) -match 'Program Manager' }
    $done += "the desktop was back after $desk s"
    Start-Sleep -Seconds 5
    return $done
}

Line "## App probe on $((Get-CimInstance Win32_OperatingSystem).Caption) build $([Environment]::OSVersion.Version.Build) $env:PROCESSOR_ARCHITECTURE"
Line ''
Screen-Shot 'screen-at-start'
Line "- windows at the start $(Top-Windows)"
$cleared = Clear-Setup
if ($cleared.Count -gt 0) {
    Line "- a Windows setup screen covered the desktop, so the probe $($cleared -join ', ')"
    Start-Sleep -Seconds 3
    Line "- windows after that $(Top-Windows)"
    Screen-Shot 'screen-cleared'
}
$setup = Get-ChildItem $Built -Filter '*-setup.exe' | Select-Object -First 1
Line "- installer $($setup.Name), $($setup.Length) bytes"
$signature = Get-AuthenticodeSignature $setup.FullName
Line "- installer signature $($signature.Status)"

[Desk]::Set($photo)
Line "- wallpaper before install $([Desk]::Wallpaper())"

function Path-Count { @(([Environment]::GetEnvironmentVariable('Path', 'User')) -split ';' | Where-Object { $_.TrimEnd('\') -ieq $folder }).Count }
function Daemon-Paths { (@(Get-CimInstance Win32_Process -Filter "Name = 'scoochw.exe'" -ErrorAction SilentlyContinue | ForEach-Object { $_.CommandLine }) -join ', ') }

if ($Programs) {
    New-Item -ItemType Directory -Force $folder | Out-Null
    foreach ($name in 'scooch', 'scoochw') {
        $from = Get-ChildItem $Programs -Filter "$name.exe" | Select-Object -First 1
        Copy-Item -Force $from.FullName (Join-Path $folder "$name.exe")
    }
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
    $old = [string]$key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    $key.SetValue('Path', ((@($old -split ';' | Where-Object { $_ }) + $folder) -join ';'), [Microsoft.Win32.RegistryValueKind]::ExpandString)
    $key.Close()
    $env:Path = "$env:Path;$folder"
    $empty = Join-Path $Out 'empty.txt'
    Set-Content -Path $empty -Value ''
    $command = Start-Process -FilePath (Join-Path $folder 'scooch.exe') -NoNewWindow -PassThru -RedirectStandardInput $empty -RedirectStandardOutput (Join-Path $Out 'old-start.txt')
    if (-not $command.WaitForExit(60000)) { Stop-Process -Id $command.Id -Force -ErrorAction SilentlyContinue }
    $started = Wait-For 20 { (Daemons) -ge 1 }
    Line "- older command install in $folder, as install.ps1 lays it out, turned Scooch on with exit $($command.ExitCode), daemon after $started s from $(Daemon-Paths)"
    Line "- login entry from the older command $(Run-Value)"
    Line "- the folder is on PATH $(Path-Count) times before the app"
    Expect "the older command install started a daemon" ((Daemons) -ge 1)
}

$took = Measure-Command { (Start-Process -FilePath $setup.FullName -ArgumentList '/S' -PassThru).WaitForExit(300000) | Out-Null }
Line "- silent install took $([int]$took.TotalSeconds) s"
Line "- installed into $folder holding $((Get-ChildItem $folder -File | ForEach-Object Name) -join ', ')"
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
Line "- user PATH holds the folder $([bool]($userPath -split ';' | Where-Object { $_.TrimEnd('\') -ieq $folder }))"
Expect "after the install the user Path holds the folder once" ((Path-Count) -eq 1)

Line "- animation effects before $([Desk]::Animations())"
[Desk]::SetAnimations($true)
Line "- animation effects after the probe turned them on $([Desk]::Animations())"
$app = Get-ChildItem $folder -Filter 'scooch-app.exe' | Select-Object -First 1
Start-Process -FilePath $app.FullName -RedirectStandardError (Join-Path $Out 'app.log')
$shown = Wait-For 40 { [bool](Find-Window) }
Line "- window showed after $shown s"
Expect "the app opened its window" ($shown -ge 0)
Start-Process -FilePath $app.FullName
Start-Sleep -Seconds 6
Line "- a second launch left $(@(Get-Process -Name 'scooch-app' -ErrorAction SilentlyContinue).Count) window process"
Expect "a second launch leaves one window process" (@(Get-Process -Name 'scooch-app' -ErrorAction SilentlyContinue).Count -eq 1)
Start-Sleep -Seconds 3
$on = Wait-For 40 { State-On }
Line "- first open turned Scooch on after $on s"
Expect "first open turned Scooch on" ($on -ge 0)
$drawn = Wait-For 40 { (Test-Path "$data\state.json") -and ((Read-Shared "$data\state.json") -match '"fingerprint": "') }
Line "- Scooch drew the desktop after $drawn s"
Expect "Scooch drew the desktop" ($drawn -ge 0)
Start-Sleep -Seconds 3
Shot 'on'
$looks = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize'
function Set-Look([int]$Light) {
    if (-not (Test-Path $looks)) { New-Item -Path $looks -Force | Out-Null }
    Set-ItemProperty -Path $looks -Name AppsUseLightTheme -Value $Light -Type DWord
    Set-ItemProperty -Path $looks -Name SystemUsesLightTheme -Value $Light -Type DWord
    [Desk]::Announce('ImmersiveColorSet')
    Start-Sleep -Seconds 3
}
Line "- the system look before the probe, apps light $((Get-ItemProperty -Path $looks -Name AppsUseLightTheme -ErrorAction SilentlyContinue).AppsUseLightTheme)"
Set-Look 1
Shot 'look-light-on'
Expect "the window is light in the light look while on" (Light-Shot 'look-light-on')
Set-Look 0
Shot 'look-dark-on'
Expect "the window is dark in the dark look while on" (Dark-Shot 'look-dark-on')
Get-CimInstance Win32_Process -Filter "Name = 'scoochw.exe'" | Where-Object { $_.CommandLine -match ' watch' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }
Start-Sleep -Seconds 2
$front = Find-Window
function Refocus {
    if (-not $front) { return }
    $hwnd = [IntPtr]$front.Current.NativeWindowHandle
    [Desk]::ShowWindow($hwnd, 6) | Out-Null
    Start-Sleep -Milliseconds 700
    [Desk]::ShowWindow($hwnd, 9) | Out-Null
    [Desk]::SetForegroundWindow($hwnd) | Out-Null
    Start-Sleep -Seconds 3
}
Refocus
Line "- stopped the daemon so the window shows a problem, daemons now $(Daemons)"
Shot 'look-dark-problem'
Expect "the window is dark in the dark look with a problem" (Dark-Shot 'look-dark-problem')
Set-Look 1
Shot 'look-light-problem'
Expect "the window is light in the light look with a problem" (Light-Shot 'look-light-problem')
Start-Process -FilePath (Join-Path $folder 'scoochw.exe') -ArgumentList 'watch' -WindowStyle Hidden
Wait-For 30 { (Daemons) -ge 1 } | Out-Null
Refocus
Line "- login entry $(Run-Value)"
Expect "the login entry starts scoochw from the install folder" ((Run-Value) -like "*$folder\scoochw.exe*watch*")
Line "- wallpaper while on $([Desk]::Wallpaper())"
Line "- daemons running $(Daemons) from $(Daemon-Paths)"
Line "- a terminal finds scooch at $((Get-Command scooch -ErrorAction SilentlyContinue).Source)"
Line "- the folder is on PATH $(Path-Count) times, copies of scooch.exe under LOCALAPPDATA $(@(Get-ChildItem $env:LOCALAPPDATA -Recurse -Filter scooch.exe -ErrorAction SilentlyContinue | Where-Object { $_.FullName -notlike '*\\Temp\\*' } | ForEach-Object FullName) -join ', ')"
Start-Sleep -Seconds 2
Line "- the app says $((Get-Content (Join-Path $Out 'app.log') -ErrorAction SilentlyContinue | Select-String 'scooch-app') -join ' ')"
$access = 'HKCU:\Software\Microsoft\Accessibility'
if (-not (Test-Path $access)) { New-Item -Path $access -Force | Out-Null }
$textBefore = (Get-ItemProperty -Path $access -Name TextScaleFactor -ErrorAction SilentlyContinue).TextScaleFactor
$sized = Find-Window
$box = $sized.Current.BoundingRectangle
$handle = [IntPtr]$sized.Current.NativeWindowHandle
function Pill-Inside {
    $names = 'Slide', 'Clouds', 'Flip', 'Quick drop', 'Turn off', 'Turn on', 'Open Startup apps', 'Start again', 'Pick a file'
    $best = 'the controls were not found'
    for ($attempt = 0; $attempt -lt 8; $attempt++) {
        $window = Find-Window
        $frame = [Desk]::Client([IntPtr]$window.Current.NativeWindowHandle)
        if (-not $frame) { Start-Sleep -Seconds 1; continue }
        $parts = @()
        $whole = $true
        $seen = 0
        foreach ($label in $names) {
            $byName = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, $label)
            $control = $window.FindAll([System.Windows.Automation.TreeScope]::Descendants, $byName) | Where-Object { $_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Button -or $_.Current.ControlType -eq [System.Windows.Automation.ControlType]::CheckBox } | Select-Object -First 1
            if (-not $control) { continue }
            $seen++
            $rect = $control.Current.BoundingRectangle
            $inside = ($rect.Left -ge $frame[0]) -and ($rect.Top -ge $frame[1]) -and ($rect.Right -le $frame[2]) -and ($rect.Bottom -le $frame[3]) -and ($rect.Width -gt 0)
            $whole = $whole -and $inside
            $parts += "$label $(if ($inside) { 'inside' } else { 'cut' })"
        }
        $best = "every control whole inside the client area $($frame -join ' ') $whole, $($parts -join ', ')"
        if ($seen -ge 5) { return $best }
        Start-Sleep -Seconds 1
    }
    return "$best, after 8 tries some controls were not found"
}
Line ''
Line '### Large text'
foreach ($scale in 150, 200) {
    Set-ItemProperty -Path $access -Name TextScaleFactor -Value $scale -Type DWord
    [Desk]::Announce('WindowMetrics')
    $edgeX = [int]$box.Width - 900
    $edgeY = [int]$box.Height - 620
    foreach ($size in @(@((640 + $edgeX), (480 + $edgeY), 'smallest'), @([int]$box.Width, [int]$box.Height, 'default'))) {
        [Desk]::SetWindowPos($handle, [IntPtr]::Zero, [int]$box.X, [int]$box.Y, $size[0], $size[1], 0x0014) | Out-Null
        Refocus
        Shot "text-$scale-$($size[2])"
        $said = Pill-Inside
        Line "- text at $scale percent, $($size[2]) size, $said"
        Expect "every control is whole inside the window at $scale percent, $($size[2]) size" ($said -like 'every control whole inside the client area * True,*' -and $said -notlike '*not found*')
    }
}
if ($null -eq $textBefore) { Remove-ItemProperty -Path $access -Name TextScaleFactor -ErrorAction SilentlyContinue } else { Set-ItemProperty -Path $access -Name TextScaleFactor -Value $textBefore -Type DWord }
[Desk]::Announce('WindowMetrics')
[Desk]::SetWindowPos($handle, [IntPtr]::Zero, [int]$box.X, [int]$box.Y, [int]$box.Width, [int]$box.Height, 0x0014) | Out-Null
[Desk]::SetForegroundWindow($handle) | Out-Null
Start-Sleep -Seconds 2
Line ''
Line '### Switches, off and removal'
$glideBefore = (Read-Shared "$data\state.json") -match '"glide": true'
Line "- $(Press 'Slide' 1)"
Start-Sleep -Seconds 2
$glideAfter = (Read-Shared "$data\state.json") -match '"glide": true'
Line "- Slide was on $glideBefore and after one press is on $glideAfter"
Expect "one press of Slide flipped it" ($glideBefore -ne $glideAfter)
Shot 'slide'

Line "- $(Press 'Turn off' 4)"
$off = Wait-For 30 { -not (State-On) }
Line "- off after $off s"
Expect "Turn off turned Scooch off" ($off -ge 0)
Start-Sleep -Seconds 3
Shot 'off'
Shot 'look-light-off'
Expect "the window is light in the light look while off" (Light-Shot 'look-light-off')
Set-Look 0
Shot 'look-dark-off'
Expect "the window is dark in the dark look while off" (Dark-Shot 'look-dark-off')
Set-Look 1
Line "- wallpaper after off $([Desk]::Wallpaper()), back to the photo $([Desk]::Wallpaper() -ieq $photo)"
Line "- login entry after off '$(Run-Value)'"
Expect "off put the photo back" ([Desk]::Wallpaper() -ieq $photo)
Expect "off removed the login entry" (-not (Run-Value))

Line "- $(Press 'Turn on' 0)"
$again = Wait-For 30 { State-On }
Line "- on again after $again s"
Expect "Turn on turned Scooch on again" ($again -ge 0)
Start-Sleep -Seconds 5

. (Join-Path $PSScriptRoot 'presses-windows.ps1')
. (Join-Path $PSScriptRoot 'tray-windows.ps1')

Get-CimInstance Win32_Process -Filter "Name = 'scoochw.exe'" | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
Start-Sleep -Seconds 2
$parked = Join-Path $data 'out\probe-absent-1.jpg'
New-Item -ItemType Directory -Force (Split-Path $parked) | Out-Null
Copy-Item $photo $parked -Force
$saved = Read-Shared "$data\state.json" | ConvertFrom-Json
$saved.on = $true
$saved.screens | Add-Member -NotePropertyName 'probe-absent' -NotePropertyValue ([pscustomobject]@{ source = $photo; original = $photo; output = $parked }) -Force
$saved | ConvertTo-Json -Depth 20 | Set-Content -Encoding utf8 "$data\state.json"
Start-Process -FilePath (Join-Path $folder 'scoochw.exe') -WindowStyle Hidden
Wait-For 30 { (Daemons) -ge 1 } | Out-Null
Start-Sleep -Seconds 3
Line "- the saved state holds a record for a screen probe-absent that no desktop shows $((Read-Shared "$data\state.json") -match 'probe-absent'), Scooch on again, daemons $(Daemons), login entry $(Run-Value)"
Expect "the saved state holds a record for a screen nobody shows before the removal" ((Read-Shared "$data\state.json") -match 'probe-absent')
function Uninstall-Entry { @(Get-ChildItem 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall' -ErrorAction SilentlyContinue | Where-Object { (Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue).DisplayName -eq 'Scooch' }).Count }
Line "- uninstall entries for Scooch before removal $(Uninstall-Entry)"
$uninstaller = Join-Path $folder 'uninstall.exe'
$took = Measure-Command {
    (Start-Process -FilePath $uninstaller -ArgumentList '/S' -PassThru).WaitForExit(300000) | Out-Null
    Wait-For 120 { -not (Test-Path (Join-Path $folder 'scooch-app.exe')) } | Out-Null
}
Start-Sleep -Seconds 3
Line "- silent removal took $([int]$took.TotalSeconds) s"
Line "- wallpaper after removal $([Desk]::Wallpaper()), back to the photo $([Desk]::Wallpaper() -ieq $photo)"
Line "- login entry after removal '$(Run-Value)', daemons running $(Daemons)"
Line "- left in the install folder $(@(Get-ChildItem $folder -ErrorAction SilentlyContinue).Count) files"
$entryGone = Wait-For 60 { (Uninstall-Entry) -eq 0 }
Line "- uninstall entries for Scooch after removal $(Uninstall-Entry), gone after $entryGone s more, scoochw processes $(@(Get-Process -Name scoochw -ErrorAction SilentlyContinue).Count)"
Expect "a removal with a screen nobody shows leaves no uninstall entry" ((Uninstall-Entry) -eq 0)
Expect "a removal with a screen nobody shows leaves no scoochw process" (@(Get-Process -Name scoochw -ErrorAction SilentlyContinue).Count -eq 0)
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
Line "- user PATH still holds the folder $([bool]($userPath -split ';' | Where-Object { $_.TrimEnd('\') -ieq $folder }))"
Expect "removal put the photo back" ([Desk]::Wallpaper() -ieq $photo)
Expect "removal left no login entry" (-not (Run-Value))
Expect "removal left no daemon" ((Daemons) -eq 0)
Expect "removal ended the icon process" (@(Get-Process -Name 'scooch-app' -ErrorAction SilentlyContinue).Count -eq 0)
Expect "removal left the install folder empty" (@(Get-ChildItem $folder -ErrorAction SilentlyContinue).Count -eq 0)
Expect "removal kept the Path entry the older command install had made" ([bool]($userPath -split ';' | Where-Object { $_.TrimEnd('\') -ieq $folder }))
Copy-Item "$data\state.json" (Join-Path $Out 'state.json') -ErrorAction SilentlyContinue

function Path-State {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment')
    try {
        if ($key.GetValueNames() -notcontains 'Path') { return 'absent' }
        return "$($key.GetValueKind('Path')) '$($key.GetValue('Path', $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames))'"
    } finally { $key.Close() }
}
function Set-PathState($Value, [Microsoft.Win32.RegistryValueKind]$Kind) {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
    if ($null -eq $Value) { $key.DeleteValue('Path', $false) } else { $key.SetValue('Path', $Value, $Kind) }
    $key.Close()
}
function Install-Plain { (Start-Process -FilePath $setup.FullName -ArgumentList '/S' -PassThru).WaitForExit(300000) | Out-Null }
function Remove-Plain {
    (Start-Process -FilePath (Join-Path $folder 'uninstall.exe') -ArgumentList '/S' -PassThru).WaitForExit(300000) | Out-Null
    Wait-For 120 { -not (Test-Path (Join-Path $folder 'scooch-app.exe')) } | Out-Null
    Start-Sleep -Seconds 2
}

$runnerKey = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment')
$runnerHad = $runnerKey.GetValueNames() -contains 'Path'
$runnerValue = if ($runnerHad) { $runnerKey.GetValue('Path', $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames) } else { $null }
$runnerKind = if ($runnerHad) { $runnerKey.GetValueKind('Path') } else { [Microsoft.Win32.RegistryValueKind]::ExpandString }
$runnerKey.Close()
$expand = [Microsoft.Win32.RegistryValueKind]::ExpandString
$plain = [Microsoft.Win32.RegistryValueKind]::String
$cases = @(
    @{ Name = 'one stock entry'; Value = '%USERPROFILE%\AppData\Local\Microsoft\WindowsApps'; Kind = $expand; Added = "ExpandString '%USERPROFILE%\AppData\Local\Microsoft\WindowsApps;$folder'" },
    @{ Name = 'an empty value'; Value = ''; Kind = $expand; Added = "ExpandString '$folder'" },
    @{ Name = 'a trailing semicolon'; Value = 'C:\One;C:\Two;'; Kind = $expand; Added = "ExpandString 'C:\One;C:\Two;$folder'" },
    @{ Name = 'a REG_SZ value'; Value = 'C:\Tools'; Kind = $plain; Added = "String 'C:\Tools;$folder'" },
    @{ Name = 'no value at all'; Value = $null; Kind = $expand; Added = "ExpandString '$folder'" }
)
foreach ($case in $cases) {
    Line ''
    Line "### Install and removal with $($case.Name) as the user Path"
    Set-PathState $case.Value $case.Kind
    $before = Path-State
    Install-Plain
    $during = Path-State
    Line "- before $before, after the install $during"
    Expect "with $($case.Name) the install appends exactly the folder" ($during -ceq $case.Added)
    Remove-Plain
    $back = Path-State
    Line "- after the removal $back"
    Expect "with $($case.Name) the removal gives the user Path back byte for byte with its kind" ($back -ceq $before)
}
Line ''
Line '### An entry the person added after the install'
Set-PathState 'C:\One' $expand
Install-Plain
Set-PathState "C:\One;$folder;C:\Later" $expand
Remove-Plain
$back = Path-State
Line "- after the removal $back"
Expect "the removal took out only Scooch's entry and one separator" ($back -ceq "ExpandString 'C:\One;C:\Later'")
Set-PathState $runnerValue $runnerKind
Expect "the runner's user Path is back" ((Path-State) -ceq $(if ($runnerHad) { "$runnerKind '$runnerValue'" } else { 'absent' }))

$marker = Join-Path $data 'reinstall.on'
function Install-Quiet { (Start-Process -FilePath $setup.FullName -ArgumentList '/S' -PassThru).WaitForExit(300000) | Out-Null }
function Remove-Quiet {
    (Start-Process -FilePath (Join-Path $folder 'uninstall.exe') -ArgumentList '/S' -PassThru).WaitForExit(300000) | Out-Null
    Wait-For 120 { -not (Test-Path (Join-Path $folder 'scooch-app.exe')) } | Out-Null
    Start-Sleep -Seconds 2
}

Line ''
Line '### Reinstall marker'
Line "- a marker from the last removal with Scooch on $(Test-Path $marker), cleared so the next pass starts with none"
Remove-Item -Force $marker -ErrorAction SilentlyContinue
Install-Quiet
Line "- installed with no marker, daemons $(Daemons), state on $(State-On)"
Remove-Quiet
Line "- removed with Scooch off, a marker left $(Test-Path $marker)"
Expect "a removal with Scooch off writes no marker" (-not (Test-Path $marker))
New-Item -ItemType Directory -Force $data | Out-Null
Set-Content -Path $marker -Value ([DateTime]::UtcNow.AddMinutes(-20).ToString('o'))
Install-Quiet
Start-Sleep -Seconds 8
Line "- installed over a marker 20 minutes old, daemons $(Daemons), marker left $(Test-Path $marker)"
Expect "a marker 20 minutes old turns nothing on" ((Daemons) -eq 0 -and -not (Test-Path $marker))
Remove-Quiet
Set-Content -Path $marker -Value ([DateTime]::UtcNow.AddMinutes(-1).ToString('o'))
Install-Quiet
$started = Wait-For 30 { (Daemons) -ge 1 }
Line "- installed over a marker 1 minute old, a daemon after $started s, marker left $(Test-Path $marker)"
Expect "a marker 1 minute old turns Scooch on again" ($started -ge 0 -and -not (Test-Path $marker))
Wait-For 30 { State-On } | Out-Null
Remove-Quiet
Line "- removed with Scooch on, a marker written $(Test-Path $marker) holding $((Get-Content -Raw $marker -ErrorAction SilentlyContinue))"
Expect "a removal with Scooch on writes a timed marker" ([bool](Get-Content -Raw $marker -ErrorAction SilentlyContinue) -and ((Get-Content -Raw $marker) -match '^\d{4}-\d{2}-\d{2}T'))

Line ''
Line '### Removal with the delete box ticked'
Install-Quiet
Start-Process -FilePath (Join-Path $folder 'uninstall.exe') | Out-Null
$checkbox = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::CheckBox)
$script:dialog = $null
$shown = Wait-For 60 {
    foreach ($window in [System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)) {
        if ($window.Current.Name -like '*Scooch*' -and $window.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $checkbox)) { $script:dialog = $window; return $true }
    }
    return $false
}
if ($script:dialog) {
    $box = $script:dialog.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $checkbox)
    Line "- the uninstaller showed '$($script:dialog.Current.Name)' after $shown s with the box '$($box.Current.Name)'"
    $box.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Screen-Shot 'uninstall-box'
    $byName = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, 'Uninstall')
    $button = $script:dialog.FindAll([System.Windows.Automation.TreeScope]::Descendants, $byName) | Where-Object { $_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Button } | Select-Object -First 1
    $button.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    $gone = Wait-For 120 { -not (Test-Path (Join-Path $folder 'scooch-app.exe')) }
    if ($gone -lt 0) {
        Screen-Shot 'uninstall-stuck'
        Line "- the removal did not finish, windows $(Top-Windows), Scooch processes $(@(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object { $_.Name -like 'scooch*' } | ForEach-Object { "$($_.Name) $($_.ProcessId) $($_.ExecutablePath)" }) -join '; ')"
    }
    foreach ($label in 'Finish', 'Close') {
        $byLabel = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, $label)
        $end = $script:dialog.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $byLabel)
        if ($end) { $end.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke(); break }
    }
    Start-Sleep -Seconds 3
    Line "- removal finished after $gone s, Scooch's data folder left $(Test-Path $data), marker left $(Test-Path $marker), the window's data folder left $(Test-Path (Join-Path $env:LOCALAPPDATA 'me.sohanbhat.scooch.window'))"
    Expect "the ticked box removed Scooch's data folder with the marker" (-not (Test-Path $data))
} else {
    Screen-Shot 'uninstall-missing'
    Line "- no uninstaller window with a box came within 60 s, windows $(Top-Windows)"
    Expect "the uninstaller showed its box" $false
}
Finish

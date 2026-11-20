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

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Drawing
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
public static class Desk {
    [DllImport("user32.dll")]
    public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr window);
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
    $graphics.CopyFromScreen([int]$box.X, [int]$box.Y, 0, 0, $bitmap.Size)
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
            $found = $window.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $byName)
            if ($found) {
                $invoke = $null
                if ($found.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$invoke)) {
                    $invoke.Invoke()
                    return "pressed $Label through UI Automation"
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

function State-On { (Test-Path "$data\state.json") -and ((Get-Content -Raw "$data\state.json") -match '"on": true') }
function Run-Value { (Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name Scooch -ErrorAction SilentlyContinue).Scooch }
function Daemons { @(Get-Process -Name scoochw -ErrorAction SilentlyContinue).Count }

Line "## App probe on $((Get-CimInstance Win32_OperatingSystem).Caption) build $([Environment]::OSVersion.Version.Build)"
Line ''
$setup = Get-ChildItem $Built -Filter '*-setup.exe' | Select-Object -First 1
Line "- installer $($setup.Name), $($setup.Length) bytes"
$signature = Get-AuthenticodeSignature $setup.FullName
Line "- installer signature $($signature.Status)"

[Desk]::Set($photo)
Line "- wallpaper before install $([Desk]::Wallpaper())"

function Path-Count { @(([Environment]::GetEnvironmentVariable('Path', 'User')) -split ';' | Where-Object { $_.TrimEnd('\') -ieq $folder }).Count }
function Daemon-Paths { (@(Get-Process -Name scoochw -ErrorAction SilentlyContinue | ForEach-Object { $_.Path }) -join ', ') }

if ($Programs) {
    New-Item -ItemType Directory -Force $folder | Out-Null
    foreach ($name in 'scooch', 'scoochw') {
        $from = Get-ChildItem $Programs -Filter "$name-*.exe" | Select-Object -First 1
        Copy-Item -Force $from.FullName (Join-Path $folder "$name.exe")
    }
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
    $old = [string]$key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    $key.SetValue('Path', ((@($old -split ';' | Where-Object { $_ }) + $folder) -join ';'), [Microsoft.Win32.RegistryValueKind]::ExpandString)
    $key.Close()
    $env:Path = "$env:Path;$folder"
    $command = Start-Process -FilePath (Join-Path $folder 'scooch.exe') -NoNewWindow -Wait -PassThru -RedirectStandardOutput (Join-Path $Out 'old-start.txt')
    $started = Wait-For 20 { (Daemons) -ge 1 }
    Line "- older command install in $folder, as install.ps1 lays it out, turned Scooch on with exit $($command.ExitCode), daemon after $started s from $(Daemon-Paths)"
    Line "- login entry from the older command $(Run-Value)"
    Line "- the folder is on PATH $(Path-Count) times before the app"
}

$took = Measure-Command { Start-Process -FilePath $setup.FullName -ArgumentList '/S' -Wait }
Line "- silent install took $([int]$took.TotalSeconds) s"
Line "- installed into $folder holding $((Get-ChildItem $folder -File | ForEach-Object Name) -join ', ')"
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
Line "- user PATH holds the folder $([bool]($userPath -split ';' | Where-Object { $_.TrimEnd('\') -ieq $folder }))"

Line "- animation effects before $([Desk]::Animations())"
[Desk]::SetAnimations($true)
Line "- animation effects after the probe turned them on $([Desk]::Animations())"
$app = Get-ChildItem $folder -Filter 'scooch-app.exe' | Select-Object -First 1
Start-Process -FilePath $app.FullName -RedirectStandardError (Join-Path $Out 'app.log')
$shown = Wait-For 40 { [bool](Find-Window) }
Line "- window showed after $shown s"
Start-Sleep -Seconds 3
$on = Wait-For 40 { State-On }
Line "- first open turned Scooch on after $on s"
$drawn = Wait-For 40 { (Test-Path "$data\state.json") -and ((Get-Content -Raw "$data\state.json") -match '"fingerprint": "') }
Line "- Scooch drew the desktop after $drawn s"
Start-Sleep -Seconds 3
Shot 'on'
Line "- login entry $(Run-Value)"
Line "- wallpaper while on $([Desk]::Wallpaper())"
Line "- daemons running $(Daemons) from $(Daemon-Paths)"
Line "- a terminal finds scooch at $((Get-Command scooch -ErrorAction SilentlyContinue).Source)"
Line "- the folder is on PATH $(Path-Count) times, copies of scooch.exe under LOCALAPPDATA $(@(Get-ChildItem $env:LOCALAPPDATA -Recurse -Filter scooch.exe -ErrorAction SilentlyContinue | Where-Object { $_.FullName -notlike '*\\Temp\\*' } | ForEach-Object FullName) -join ', ')"
Start-Sleep -Seconds 2
Line "- the app says $((Get-Content (Join-Path $Out 'app.log') -ErrorAction SilentlyContinue | Select-String 'scooch-app') -join ' ')"
$glideBefore = (Get-Content -Raw "$data\state.json") -match '"glide": true'
Line "- $(Press 'Slide' 1)"
Start-Sleep -Seconds 2
$glideAfter = (Get-Content -Raw "$data\state.json") -match '"glide": true'
Line "- Slide was on $glideBefore and after one press is on $glideAfter"
Shot 'slide'

Line "- $(Press 'Turn off' 4)"
$off = Wait-For 30 { -not (State-On) }
Line "- off after $off s"
Start-Sleep -Seconds 3
Shot 'off'
Line "- wallpaper after off $([Desk]::Wallpaper()), back to the photo $([Desk]::Wallpaper() -ieq $photo)"
Line "- login entry after off '$(Run-Value)'"

Line "- $(Press 'Turn on' 0)"
$again = Wait-For 30 { State-On }
Line "- on again after $again s"
Start-Sleep -Seconds 5

$window = Find-Window
if ($window) {
    $window.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).Close()
}
$closed = Wait-For 15 { @(Get-Process -Name 'scooch-app' -ErrorAction SilentlyContinue).Count -eq 0 }
Line "- closing the window ended the app process after $closed s, daemons still running $(Daemons)"

$uninstaller = Join-Path $folder 'uninstall.exe'
$took = Measure-Command { Start-Process -FilePath $uninstaller -ArgumentList '/S' -Wait }
Start-Sleep -Seconds 5
Line "- silent removal took $([int]$took.TotalSeconds) s"
Line "- wallpaper after removal $([Desk]::Wallpaper()), back to the photo $([Desk]::Wallpaper() -ieq $photo)"
Line "- login entry after removal '$(Run-Value)', daemons running $(Daemons)"
Line "- left in the install folder $(@(Get-ChildItem $folder -ErrorAction SilentlyContinue).Count) files"
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
Line "- user PATH still holds the folder $([bool]($userPath -split ';' | Where-Object { $_.TrimEnd('\') -ieq $folder }))"
Copy-Item "$data\state.json" (Join-Path $Out 'state.json') -ErrorAction SilentlyContinue
exit 0

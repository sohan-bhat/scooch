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
public static class Desk {
    [DllImport("user32.dll")]
    public static extern bool SetProcessDPIAware();
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

function State-On { (Test-Path "$data\state.json") -and ((Get-Content -Raw "$data\state.json") -match '"on": true') }
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
        $from = Get-ChildItem $Programs -Filter "$name-*.exe" | Select-Object -First 1
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
}

$took = Measure-Command { (Start-Process -FilePath $setup.FullName -ArgumentList '/S' -PassThru).WaitForExit(300000) | Out-Null }
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
Start-Process -FilePath $app.FullName
Start-Sleep -Seconds 6
Line "- a second launch left $(@(Get-Process -Name 'scooch-app' -ErrorAction SilentlyContinue).Count) window process"
Start-Sleep -Seconds 3
$on = Wait-For 40 { State-On }
Line "- first open turned Scooch on after $on s"
$drawn = Wait-For 40 { (Test-Path "$data\state.json") -and ((Get-Content -Raw "$data\state.json") -match '"fingerprint": "') }
Line "- Scooch drew the desktop after $drawn s"
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
Set-Look 0
Shot 'look-dark-on'
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
Set-Look 1
Shot 'look-light-problem'
Start-Process -FilePath (Join-Path $folder 'scoochw.exe') -ArgumentList 'watch' -WindowStyle Hidden
Wait-For 30 { (Daemons) -ge 1 } | Out-Null
Refocus
Line "- login entry $(Run-Value)"
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
    for ($attempt = 0; $attempt -lt 6; $attempt++) {
        $window = Find-Window
        $frame = $window.Current.BoundingRectangle
        $parts = @()
        $whole = $true
        $seen = 0
        foreach ($label in $names) {
            $byName = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, $label)
            $control = $window.FindAll([System.Windows.Automation.TreeScope]::Descendants, $byName) | Where-Object { $_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Button -or $_.Current.ControlType -eq [System.Windows.Automation.ControlType]::CheckBox } | Select-Object -First 1
            if (-not $control) { continue }
            $seen++
            $rect = $control.Current.BoundingRectangle
            $inside = ($rect.Left -ge $frame.Left) -and ($rect.Top -ge $frame.Top) -and ($rect.Right -le $frame.Right) -and ($rect.Bottom -le $frame.Bottom) -and ($rect.Width -gt 0)
            $whole = $whole -and $inside
            $parts += "$label $(if ($inside) { 'inside' } else { 'cut' })"
        }
        if ($seen -ge 3) { return "every control whole inside the window $whole, $($parts -join ', ')" }
        Start-Sleep -Seconds 1
    }
    return 'the controls were not found'
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
        Line "- text at $scale percent, $($size[2]) size, $(Pill-Inside)"
    }
}
if ($null -eq $textBefore) { Remove-ItemProperty -Path $access -Name TextScaleFactor -ErrorAction SilentlyContinue } else { Set-ItemProperty -Path $access -Name TextScaleFactor -Value $textBefore -Type DWord }
[Desk]::Announce('WindowMetrics')
[Desk]::SetWindowPos($handle, [IntPtr]::Zero, [int]$box.X, [int]$box.Y, [int]$box.Width, [int]$box.Height, 0x0014) | Out-Null
[Desk]::SetForegroundWindow($handle) | Out-Null
Start-Sleep -Seconds 2
Line ''
Line '### Switches, off and removal'
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
Shot 'look-light-off'
Set-Look 0
Shot 'look-dark-off'
Set-Look 1
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
$took = Measure-Command {
    (Start-Process -FilePath $uninstaller -ArgumentList '/S' -PassThru).WaitForExit(300000) | Out-Null
    Wait-For 120 { -not (Test-Path (Join-Path $folder 'scooch-app.exe')) } | Out-Null
}
Start-Sleep -Seconds 3
Line "- silent removal took $([int]$took.TotalSeconds) s"
Line "- wallpaper after removal $([Desk]::Wallpaper()), back to the photo $([Desk]::Wallpaper() -ieq $photo)"
Line "- login entry after removal '$(Run-Value)', daemons running $(Daemons)"
Line "- left in the install folder $(@(Get-ChildItem $folder -ErrorAction SilentlyContinue).Count) files"
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
Line "- user PATH still holds the folder $([bool]($userPath -split ';' | Where-Object { $_.TrimEnd('\') -ieq $folder }))"
Copy-Item "$data\state.json" (Join-Path $Out 'state.json') -ErrorAction SilentlyContinue

function Raw-Path {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment')
    $value = $key.GetValue('Path', $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    $key.Close()
    return $value
}

function Set-RawPath($Value) {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
    if ($null -eq $Value) { $key.DeleteValue('Path', $false) } else { $key.SetValue('Path', $Value, [Microsoft.Win32.RegistryValueKind]::ExpandString) }
    $key.Close()
}

$runnerPath = Raw-Path
foreach ($stock in @('%USERPROFILE%\AppData\Local\Microsoft\WindowsApps', '')) {
    Line ''
    Line "### Install with the user Path set to '$stock'"
    Set-RawPath $stock
    (Start-Process -FilePath $setup.FullName -ArgumentList '/S' -PassThru).WaitForExit(300000) | Out-Null
    $after = Raw-Path
    $want = @(@($stock -split ';' | Where-Object { $_ }) + @($folder)) -join ';'
    Line "- after the install the user Path is '$after', entries $(@($after -split ';' | Where-Object { $_ }).Count), as expected $([string]::Equals($after, $want, [StringComparison]::Ordinal))"
    (Start-Process -FilePath (Join-Path $folder 'uninstall.exe') -ArgumentList '/S' -PassThru).WaitForExit(300000) | Out-Null
    Wait-For 120 { -not (Test-Path (Join-Path $folder 'scooch-app.exe')) } | Out-Null
    Start-Sleep -Seconds 2
    $back = Raw-Path
    Line "- after the removal the user Path is '$back', the same bytes as before $([string]::Equals($back, $stock, [StringComparison]::Ordinal))"
}
Set-RawPath $runnerPath
Line "- the runner's user Path is back $([string]::Equals((Raw-Path), $runnerPath, [StringComparison]::Ordinal))"

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
New-Item -ItemType Directory -Force $data | Out-Null
Set-Content -Path $marker -Value ([DateTime]::UtcNow.AddMinutes(-20).ToString('o'))
Install-Quiet
Start-Sleep -Seconds 8
Line "- installed over a marker 20 minutes old, daemons $(Daemons), marker left $(Test-Path $marker)"
Remove-Quiet
Set-Content -Path $marker -Value ([DateTime]::UtcNow.AddMinutes(-1).ToString('o'))
Install-Quiet
$started = Wait-For 30 { (Daemons) -ge 1 }
Line "- installed over a marker 1 minute old, a daemon after $started s, marker left $(Test-Path $marker)"
Wait-For 30 { State-On } | Out-Null
Remove-Quiet
Line "- removed with Scooch on, a marker written $(Test-Path $marker) holding $((Get-Content -Raw $marker -ErrorAction SilentlyContinue))"

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
    foreach ($label in 'Finish', 'Close') {
        $byLabel = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, $label)
        $end = $script:dialog.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $byLabel)
        if ($end) { $end.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke(); break }
    }
    Start-Sleep -Seconds 3
    Line "- removal finished after $gone s, Scooch's data folder left $(Test-Path $data), marker left $(Test-Path $marker), the window's data folder left $(Test-Path (Join-Path $env:LOCALAPPDATA 'me.sohanbhat.scooch.window'))"
} else {
    Screen-Shot 'uninstall-missing'
    Line "- no uninstaller window with a box came within 60 s, windows $(Top-Windows)"
}
exit 0

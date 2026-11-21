Line ''
Line '### Every control pressed once'
$photos = (Resolve-Path 'assets/photos').Path
$pics = Join-Path $env:USERPROFILE 'probe-pictures'
New-Item -ItemType Directory -Force $pics | Out-Null
$canoe = Join-Path $pics 'probe-canoe.jpg'
$sky = Join-Path $pics 'probe-sky.jpg'
$pick = Join-Path $pics 'probe-pick.jpg'
$notes = Join-Path $pics 'probe-notes.jpg'
$flip = Join-Path $pics 'probe-flip.png'
Copy-Item (Join-Path $photos 'lone-canoe.jpg') $canoe -Force
Copy-Item (Join-Path $photos 'balloon-sky.jpg') $sky -Force
Copy-Item (Join-Path $photos 'fox.jpg') $pick -Force
Set-Content -Path $notes -Value 'these are words and no picture'
Copy-Item (Join-Path $PSScriptRoot 'flip-left.png') $flip -Force
$appdata = Join-Path $env:APPDATA 'me.sohanbhat.scooch.window'
New-Item -ItemType Directory -Force $appdata | Out-Null
ConvertTo-Json -Compress @($canoe, $sky, $photo) | Set-Content -Path (Join-Path $appdata 'pictures.json')
$unreadable = (Get-Content -Raw 'crates/scooch-app/ui/src/copy/en.json' | ConvertFrom-Json).failure.use
$scooch = Join-Path $folder 'scooch.exe'
$app = Get-ChildItem $folder -Filter 'scooch-app.exe' | Select-Object -First 1

function Saved { Get-Content -Raw "$data\state.json" | ConvertFrom-Json }
function Plain([string]$Path) { if (-not $Path) { return '' }; return $Path -replace '^\\\\\?\\', '' }
function Sources { @((Saved).screens.PSObject.Properties.Value | ForEach-Object { Plain $_.source } | Sort-Object -Unique) }
function Names([string]$Path) { $found = @(Sources); return ($found.Count -eq 1) -and ($found[0] -ieq $Path) }
function Digest { $path = [Desk]::Wallpaper(); if ($path -and (Test-Path $path)) { return (Get-FileHash -Algorithm MD5 -Path $path).Hash } else { return '' } }
function Ours { $path = [Desk]::Wallpaper(); return [bool]$path -and ($path -like "$data\out\*") }
function Fresh-From([string]$Path, [string]$Before) { return (Names $Path) -and (Ours) -and ((Digest) -ne $Before) }
function Status-Text { return ((& $scooch status 2>&1) -join '|') }
function Alive-On { return ((Status-Text) -like 'Scooch is on.|*') -and ((Daemons) -ge 1) }
function Every-Screen([scriptblock]$Test) { $all = @((Saved).screens.PSObject.Properties.Value); return ($all.Count -gt 0) -and (@($all | Where-Object { -not (& $Test $_) }).Count -eq 0) }
function Control([string]$Label) {
    $window = Find-Window
    if (-not $window) { return $null }
    $byName = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, $Label)
    $all = @($window.FindAll([System.Windows.Automation.TreeScope]::Descendants, $byName))
    foreach ($found in $all) {
        $pattern = $null
        if ($found.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern, [ref]$pattern) -or $found.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern, [ref]$pattern)) { return $found }
    }
    if ($all.Count -gt 0) { return $all[0] }
    return $null
}
function Page-Text {
    $window = Find-Window
    if (-not $window) { return '' }
    $byType = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::Document)
    foreach ($page in @($window.FindAll([System.Windows.Automation.TreeScope]::Descendants, $byType))) {
        $text = $null
        if ($page.TryGetCurrentPattern([System.Windows.Automation.TextPattern]::Pattern, [ref]$text)) { return $text.DocumentRange.GetText(-1) }
    }
    return ''
}
function Chosen([string]$Label) {
    $found = Control $Label
    if (-not $found) { return $false }
    $selection = $null
    if ($found.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern, [ref]$selection)) { return [bool]$selection.Current.IsSelected }
    $toggle = $null
    if ($found.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern, [ref]$toggle)) { return $toggle.Current.ToggleState -eq [System.Windows.Automation.ToggleState]::On }
    return $false
}
function States([string[]]$Labels) {
    return ($Labels | ForEach-Object { if (Control $_) { "$_ $(if (Chosen $_) { 'on' } else { 'off' })" } else { "$_ missing" } }) -join '; '
}
function Click-Control([string]$Label) {
    $window = Find-Window
    if (-not $window) { return "no window to click $Label in" }
    [Desk]::SetForegroundWindow([IntPtr]$window.Current.NativeWindowHandle) | Out-Null
    Start-Sleep -Milliseconds 400
    $found = Control $Label
    if (-not $found) { return "no control named $Label" }
    $selection = $null
    if ($found.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern, [ref]$selection)) {
        try { $selection.Select() } catch { }
        Start-Sleep -Milliseconds 2500
        if (Chosen $Label) { return "selected $Label through UI Automation" }
    }
    try { $found.SetFocus() } catch { }
    Start-Sleep -Milliseconds 300
    [System.Windows.Forms.SendKeys]::SendWait(' ')
    Start-Sleep -Milliseconds 2500
    if (Chosen $Label) { return "pressed $Label with the space key" }
    $found = Control $Label
    $rect = $found.Current.BoundingRectangle
    if ($rect.Width -le 0) { return "$Label has no place on the screen" }
    [Desk]::Click([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2))
    return "clicked $Label with the mouse"
}
function Dialog {
    $process = Get-Process -Name 'scooch-app' -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $process) { return $null }
    $byProcess = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ProcessIdProperty, $process.Id)
    $byClass = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ClassNameProperty, '#32770')
    $both = New-Object System.Windows.Automation.AndCondition($byProcess, $byClass)
    $root = [System.Windows.Automation.AutomationElement]::RootElement
    $found = $root.FindFirst([System.Windows.Automation.TreeScope]::Children, $both)
    if (-not $found) {
        $window = Find-Window
        if ($window) { $found = $window.FindFirst([System.Windows.Automation.TreeScope]::Children, $byClass) }
    }
    return $found
}
function Dialog-Part([object]$Dialog, [string]$Id) {
    $byId = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::AutomationIdProperty, $Id)
    return @($Dialog.FindAll([System.Windows.Automation.TreeScope]::Descendants, $byId))
}
function Dialog-Open([object]$Dialog) {
    $open = Dialog-Part $Dialog '1' | Select-Object -First 1
    $invoke = $null
    if ($open -and $open.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$invoke)) { $invoke.Invoke(); return $true }
    return $false
}
function Dialog-Item([object]$Dialog, [string]$File) {
    $leaf = [IO.Path]::GetFileName($File)
    $base = [IO.Path]::GetFileNameWithoutExtension($File)
    $byType = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::ListItem)
    return @($Dialog.FindAll([System.Windows.Automation.TreeScope]::Descendants, $byType)) | Where-Object { $_.Current.Name -eq $leaf -or $_.Current.Name -eq $base } | Select-Object -First 1
}
function Pick-File([string]$File) {
    $pressed = Press 'Pick a file' 0
    $dialog = $null
    for ($step = 0; $step -lt 30 -and -not $dialog; $step++) { Start-Sleep -Milliseconds 500; $dialog = Dialog }
    if (-not $dialog) { return "$pressed, and no file window opened" }
    $title = $dialog.Current.Name
    Start-Sleep -Seconds 1
    $item = Dialog-Item $dialog $File
    $how = 'its file list'
    if (-not $item) {
        $box = Dialog-Part $dialog '1148' | Where-Object { $_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Edit } | Select-Object -First 1
        $value = $null
        if ($box -and $box.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$value)) {
            $value.SetValue((Split-Path $File))
            Dialog-Open $dialog | Out-Null
            $how = 'the folder typed into its file name box, then its file list'
            for ($step = 0; $step -lt 20 -and -not $item; $step++) { Start-Sleep -Milliseconds 500; $item = Dialog-Item $dialog $File }
        }
    }
    $selection = $null
    if ($item -and $item.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern, [ref]$selection)) {
        $selection.Select()
        Start-Sleep -Seconds 1
        $box = Dialog-Part $dialog '1148' | Where-Object { $_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Edit } | Select-Object -First 1
        $value = $null
        $shows = ''
        if ($box -and $box.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$value)) { $shows = $value.Current.Value }
        Dialog-Open $dialog | Out-Null
        $how = "$how, the file name box reading '$shows', and its Open button"
    } else {
        [Desk]::SetForegroundWindow([IntPtr]$dialog.Current.NativeWindowHandle) | Out-Null
        Start-Sleep -Milliseconds 500
        [System.Windows.Forms.SendKeys]::SendWait(($File -replace '([+^%~(){}\[\]])', '{$1}') + '~')
        $how = 'typed keys and Enter, since its file list showed no such file'
    }
    $closed = Wait-For 15 { -not (Dialog) }
    if ($closed -lt 0) { return "the file window '$title' stayed open after $File through $how" }
    return "the file window '$title' took $File through $how and closed"
}
function Press-Switch([string]$Label) { Press $Label 0 | Out-Null; Start-Sleep -Seconds 1 }

$window = Find-Window
if ($window) { $window.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).Close() }
Wait-For 15 { @(Get-Process -Name 'scooch-app' -ErrorAction SilentlyContinue).Count -eq 0 } | Out-Null
Start-Process -FilePath $app.FullName -RedirectStandardError (Join-Path $Out 'app-controls.log')
Wait-For 40 { [bool](Find-Window) } | Out-Null
Start-Sleep -Seconds 4
$names = 'probe canoe', 'probe sky', 'golden tree', 'Slide', 'Clouds', 'Flip', 'Quick drop'
Line "- the row and switches read $(States $names)"

Line ''
Line '#### A picture in the row and the way back'
$before = Digest
Line "- $(Click-Control 'probe canoe')"
Wait-For 40 { Fresh-From $canoe $before } | Out-Null
Start-Sleep -Seconds 2
Line "- the row reads $(States 'probe canoe', 'golden tree'), the saved state names $((Sources) -join ' '), the wallpaper is $([Desk]::Wallpaper())"
Expect "pressing probe canoe in the row marks it as chosen" (Chosen 'probe canoe')
Expect "the saved state names probe canoe" (Names $canoe)
Expect "the desktop shows a new output made from probe canoe" (Fresh-From $canoe $before)
$before = Digest
Line "- $(Click-Control 'golden tree')"
Wait-For 40 { Fresh-From $photo $before } | Out-Null
Start-Sleep -Seconds 2
Line "- the row reads $(States 'probe canoe', 'golden tree'), the saved state names $((Sources) -join ' '), the wallpaper is $([Desk]::Wallpaper())"
Expect "the way back marks golden tree as chosen and probe canoe no more" ((Chosen 'golden tree') -and -not (Chosen 'probe canoe'))
Expect "the way back saves golden tree" (Names $photo)
Expect "the way back shows a new output made from golden tree" (Fresh-From $photo $before)

Line ''
Line '#### Pick a file'
$before = Digest
$picked = Pick-File $pick
Line "- $picked"
Wait-For 40 { Fresh-From $pick $before } | Out-Null
Start-Sleep -Seconds 2
Line "- the row reads $(States 'probe pick'), the saved state names $((Sources) -join ' '), the wallpaper is $([Desk]::Wallpaper())"
Expect "Pick a file opened the system's file window, which took the photo and closed" ($picked -like '*took*and closed')
Expect "the picked photo joins the row as chosen" (Chosen 'probe pick')
Expect "the saved state names the picked photo" (Names $pick)
Expect "the desktop shows a new output made from the picked photo" (Fresh-From $pick $before)
$before = Digest
$shownBefore = [Desk]::Wallpaper()
$picked = Pick-File $notes
Line "- $picked"
Start-Sleep -Seconds 4
$page = Page-Text
$sentence = $page.Contains($unreadable) -or [bool](Control $unreadable)
Line "- the page text reads $(($page -replace '\s+', ' ').Trim())"
Line "- the window shows the sentence for an unreadable picture $sentence, the copy file says '$unreadable', the saved state names $((Sources) -join ' '), the wallpaper is $([Desk]::Wallpaper())"
Expect "the file window also took a file that is no picture" ($picked -like '*took*and closed')
Expect "a picked file that is no picture shows the window's sentence for it" $sentence
Expect "a picked file that is no picture leaves the desktop and the saved picture as they were" (([Desk]::Wallpaper() -ieq $shownBefore) -and ((Digest) -eq $before) -and (Names $pick))

Line ''
Line '#### Clouds on a sky photo, with the icon column full'
$desk = [Environment]::GetFolderPath('Desktop')
1..40 | ForEach-Object { Set-Content -Path (Join-Path $desk ('probe-icon-{0:d2}.txt' -f $_)) -Value '' }
Start-Sleep -Seconds 6
$before = Digest
Line "- $(Click-Control 'probe sky')"
Wait-For 40 { Fresh-From $sky $before } | Out-Null
Start-Sleep -Seconds 5
$withClouds = Digest
Line "- clouds saved as $((Saved).options.scenery), scooch status says $(Status-Text)"
Press-Switch 'Clouds'
Wait-For 40 { ((Saved).options.scenery -eq 'off') -and ((Digest) -ne $withClouds) } | Out-Null
Start-Sleep -Seconds 2
$withoutClouds = Digest
Line "- after one press clouds saved as $((Saved).options.scenery), the switch reads $(States 'Clouds'), scooch status says $(Status-Text)"
Expect "one press of Clouds saves clouds off" ((Saved).options.scenery -eq 'off')
Expect "the Clouds switch reads off" (-not (Chosen 'Clouds'))
Expect "scooch status drops the scenery line" ((Status-Text) -notlike '*Scenery  on*')
Expect "the sky photo without clouds is a new output unlike the one with them" ($withoutClouds -ne $withClouds)
Press-Switch 'Clouds'
Wait-For 40 { ((Saved).options.scenery -ne 'off') -and ((Digest) -ne $withoutClouds) } | Out-Null
Start-Sleep -Seconds 2
Line "- after the second press clouds saved as $((Saved).options.scenery), the switch reads $(States 'Clouds'), scooch status says $(Status-Text)"
Expect "the second press of Clouds saves clouds on" ((Saved).options.scenery -ne 'off')
Expect "the Clouds switch reads on again" (Chosen 'Clouds')
Expect "scooch status shows the scenery line again" ((Status-Text) -like '*Scenery  on*')
Expect "the sky photo with clouds again is a new output unlike the one without" ((Digest) -ne $withoutClouds)

foreach ($pair in @(@('Slide', 'glide'), @('Quick drop', 'predict'))) {
    $label = $pair[0]
    $key = $pair[1]
    Line ''
    Line "#### $label"
    if (-not (Control $label)) {
        Line "- $label is not shown on this system, so it is not pressed here"
        continue
    }
    $was = [bool](Saved).$key
    Press-Switch $label
    Wait-For 10 { [bool](Saved).$key -ne $was } | Out-Null
    Start-Sleep -Seconds 2
    Line "- $label was saved as $was and after one press as $([bool](Saved).$key), the switch reads $(States $label), scooch status says $(Status-Text)"
    Expect "one press of $label flips its saved value" ([bool](Saved).$key -ne $was)
    Expect "the $label switch reads $(if ($was) { 'off' } else { 'on' })" ((Chosen $label) -ne $was)
    Expect "after $label scooch status still says on and the daemon is alive" (Alive-On)
    Press-Switch $label
    Wait-For 10 { [bool](Saved).$key -eq $was } | Out-Null
    Start-Sleep -Seconds 2
    Line "- after the second press $label is saved as $([bool](Saved).$key), the switch reads $(States $label), scooch status says $(Status-Text)"
    Expect "the second press of $label brings its saved value back" ([bool](Saved).$key -eq $was)
    Expect "the $label switch reads $(if ($was) { 'on' } else { 'off' }) again" ((Chosen $label) -eq $was)
    Expect "after the second press of $label scooch status still says on and the daemon is alive" (Alive-On)
}

Line ''
Line '#### Flip with the icon column full'
& $scooch use $flip *> (Join-Path $Out 'flip-use.txt')
function Helps { (Names $flip) -and (Ours) -and (Every-Screen { param($entry) [bool]$entry.flip_helps }) -and ((Status-Text) -like '*Run scooch flip*') }
Wait-For 60 { Helps } | Out-Null
Start-Sleep -Seconds 2
$before = Digest
Line "- with $(@(Get-ChildItem $desk -Filter 'probe-icon-*').Count) files on the Desktop and the subject on the left, flip_helps $((@((Saved).screens.PSObject.Properties.Value | ForEach-Object { [bool]$_.flip_helps })) -join ' '), mirror $((Saved).options.mirror), scooch status says $(Status-Text)"
Expect "before Flip the saved state says flipping would help and scooch status shows the flip hint" (Helps)
Press-Switch 'Flip'
function Mirrored { ((Saved).options.mirror -eq $true) -and (Every-Screen { param($entry) [bool]$entry.placement.mirrored }) -and ((Digest) -ne $before) }
Wait-For 60 { Mirrored } | Out-Null
Start-Sleep -Seconds 2
$flippedOut = Digest
Line "- after one press of Flip mirror is saved as $((Saved).options.mirror), the switch reads $(States 'Flip'), the placement is mirrored $((@((Saved).screens.PSObject.Properties.Value | ForEach-Object { [bool]$_.placement.mirrored })) -join ' '), scooch status says $(Status-Text)"
Expect "one press of Flip saves flip on" ((Saved).options.mirror -eq $true)
Expect "the Flip switch reads on" (Chosen 'Flip')
Expect "the daemon mirrored the picture into a new output" (Mirrored)
Expect "scooch status drops the flip hint" ((Status-Text) -notlike '*Run scooch flip*')
Press-Switch 'Flip'
function Unmirrored { ((Saved).options.mirror -eq $false) -and (Every-Screen { param($entry) -not [bool]$entry.placement.mirrored }) -and ((Digest) -ne $flippedOut) }
Wait-For 60 { Unmirrored } | Out-Null
Start-Sleep -Seconds 2
Line "- after the second press mirror is saved as $((Saved).options.mirror), the switch reads $(States 'Flip'), scooch status says $(Status-Text)"
Expect "the second press of Flip saves flip off" ((Saved).options.mirror -eq $false)
Expect "the Flip switch reads off again" (-not (Chosen 'Flip'))
Expect "the daemon put the picture back unmirrored in a new output" (Unmirrored)
Get-ChildItem $desk -Filter 'probe-icon-*' | Remove-Item -Force
Start-Sleep -Seconds 3
Line "- the Desktop files of the Flip case are removed, $(@(Get-ChildItem $desk -Filter 'probe-icon-*').Count) left"

Line ''
Line '#### A picture pressed while Scooch is off'
Line "- $(Press 'Turn off' 0)"
Wait-For 30 { -not (State-On) } | Out-Null
Wait-For 30 { (Daemons) -eq 0 } | Out-Null
Start-Sleep -Seconds 3
$shownOff = [Desk]::Wallpaper()
$stateSum = (Get-FileHash -Algorithm MD5 -Path "$data\state.json").Hash
Line "- off, wallpaper $shownOff, login entry '$(Run-Value)', daemons $(Daemons)"
Line "- $(Click-Control 'probe canoe')"
Start-Sleep -Seconds 5
Line "- the row reads $(States 'probe canoe', 'golden tree'), wallpaper $([Desk]::Wallpaper()), login entry '$(Run-Value)', daemons $(Daemons), selection file $(Get-Content -Raw (Join-Path $appdata 'selected.json') -ErrorAction SilentlyContinue)"
Expect "a picture pressed while off is marked as chosen" (Chosen 'probe canoe')
Expect "pressed while off the desktop keeps its wallpaper" ([Desk]::Wallpaper() -ieq $shownOff)
Expect "pressed while off no login entry is written" (-not (Run-Value))
Expect "pressed while off no daemon runs" ((Daemons) -eq 0)
Expect "pressed while off Scooch's saved state is untouched" ((Get-FileHash -Algorithm MD5 -Path "$data\state.json").Hash -eq $stateSum)
$window = Find-Window
if ($window) { $window.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).Close() }
Wait-For 15 { @(Get-Process -Name 'scooch-app' -ErrorAction SilentlyContinue).Count -eq 0 } | Out-Null
Start-Process -FilePath $app.FullName -RedirectStandardError (Join-Path $Out 'app-controls-again.log')
Wait-For 40 { [bool](Find-Window) } | Out-Null
Start-Sleep -Seconds 4
Line "- after closing and opening the window the row reads $(States 'probe canoe', 'golden tree')"
Expect "the selection survives closing and opening the window" (Chosen 'probe canoe')
Expect "opening the window again with a selection turned nothing on" ((Daemons) -eq 0)
$before = (Get-FileHash -Algorithm MD5 -Path $shownOff).Hash
Line "- $(Press 'Turn on' 0)"
Wait-For 40 { Fresh-From $canoe $before } | Out-Null
Start-Sleep -Seconds 3
Line "- after Turn on the saved state names $((Sources) -join ' '), the wallpaper is $([Desk]::Wallpaper()), login entry '$(Run-Value)', daemons $(Daemons)"
Expect "Turn on after the press shows that picture" (Fresh-From $canoe $before)
Expect "Turn on after the press writes the login entry" ([bool](Run-Value))
Line ''

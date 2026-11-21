param(
    [Parameter(Mandatory = $true)][ValidateSet('installed', 'removing')][string]$Step,
    [Parameter(Mandatory = $true)][string]$Folder,
    [string]$WasRunning = 'no'
)

$ErrorActionPreference = 'Continue'
$Folder = $Folder.TrimEnd('\')
$marker = Join-Path $env:LOCALAPPDATA 'Scooch\reinstall.on'
$state = Join-Path $env:LOCALAPPDATA 'Scooch\state.json'
$record = 'HKCU:\Software\Scooch'

function Read-Raw {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment')
    if (-not $key) { return $null }
    try {
        if ($key.GetValueNames() -notcontains 'Path') { return $null }
        return [pscustomobject]@{
            Value = [string]$key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
            Kind = $key.GetValueKind('Path')
        }
    } finally {
        $key.Close()
    }
}

function Announce {
    [Environment]::SetEnvironmentVariable('SCOOCH_PATH_REFRESH', '1', 'User')
    [Environment]::SetEnvironmentVariable('SCOOCH_PATH_REFRESH', $null, 'User')
}

function Write-Raw([string]$Value, [Microsoft.Win32.RegistryValueKind]$Kind) {
    $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Environment')
    $key.SetValue('Path', $Value, $Kind)
    $key.Close()
    Announce
}

function Remove-Raw {
    $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Environment')
    $key.DeleteValue('Path', $false)
    $key.Close()
    Announce
}

function Same([string]$A, [string]$B) {
    return $A.TrimEnd('\').ToLowerInvariant() -eq $B.TrimEnd('\').ToLowerInvariant()
}

function Recorded([string]$Name) {
    return (Get-ItemProperty -Path $record -Name $Name -ErrorAction SilentlyContinue).$Name
}

function Holds([string]$Value) {
    return @($Value.Split(';') | Where-Object { $_ -and (Same $_ $Folder) }).Count -gt 0
}

function Fresh-Marker {
    if (-not (Test-Path $marker)) { return $false }
    $written = [DateTime]::MinValue
    $text = [string](Get-Content -Raw -Path $marker -ErrorAction SilentlyContinue)
    $read = [DateTime]::TryParse($text.Trim(), [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::RoundtripKind, [ref]$written)
    Remove-Item -Force $marker -ErrorAction SilentlyContinue
    if (-not $read) { return $false }
    $age = ([DateTime]::UtcNow - $written.ToUniversalTime()).TotalMinutes
    return ($age -ge -1) -and ($age -lt 10)
}

if ($Step -eq 'installed') {
    $found = Read-Raw
    $value = if ($found) { $found.Value } else { '' }
    if (-not (Holds $value)) {
        $joint = if ($value -eq '' -or $value.EndsWith(';')) { '' } else { ';' }
        $appended = $joint + $Folder
        $kind = if ($found) { $found.Kind } else { [Microsoft.Win32.RegistryValueKind]::ExpandString }
        Write-Raw ($value + $appended) $kind
        if (-not (Test-Path $record)) { New-Item -Path $record | Out-Null }
        Set-ItemProperty -Path $record -Name PathAppended -Value $appended
        Set-ItemProperty -Path $record -Name PathExisted -Value ([int][bool]$found) -Type DWord
    }
    $daemon = Join-Path $Folder 'scoochw.exe'
    $fresh = Fresh-Marker
    if ($WasRunning -eq 'yes') {
        Start-Process -FilePath $daemon -ArgumentList 'watch' -WindowStyle Hidden
    } elseif ($fresh) {
        Start-Process -FilePath $daemon -WindowStyle Hidden
    }
    exit 0
}

$daemon = Join-Path $Folder 'scoochw.exe'
$on = (Test-Path $state) -and ([string](Get-Content -Raw -Path $state -ErrorAction SilentlyContinue) -match '"on":\s*true')
if ($on) {
    New-Item -ItemType Directory -Force -Path (Split-Path $marker) | Out-Null
    Set-Content -Path $marker -Value ([DateTime]::UtcNow.ToString('o'))
}
if (Test-Path $daemon) {
    $off = Start-Process -FilePath $daemon -ArgumentList 'off', '--final' -WindowStyle Hidden -PassThru
    if (-not $off.WaitForExit(60000)) {
        Stop-Process -Id $off.Id -Force -ErrorAction SilentlyContinue
    }
}
$appended = [string](Recorded 'PathAppended')
if ($appended) {
    $existed = [int](Recorded 'PathExisted')
    $found = Read-Raw
    if ($found) {
        $value = $found.Value
        if ($value.EndsWith($appended)) {
            $rest = $value.Substring(0, $value.Length - $appended.Length)
        } else {
            $parts = [System.Collections.Generic.List[string]]$value.Split(';')
            $at = -1
            for ($index = 0; $index -lt $parts.Count; $index++) {
                if ($parts[$index] -and (Same $parts[$index] $Folder)) { $at = $index; break }
            }
            if ($at -ge 0) { $parts.RemoveAt($at) }
            $rest = $parts -join ';'
        }
        if ($rest -eq '' -and $existed -eq 0) { Remove-Raw } else { Write-Raw $rest $found.Kind }
    }
    Remove-ItemProperty -Path $record -Name PathAppended, PathExisted -ErrorAction SilentlyContinue
    if (@((Get-Item -Path $record -ErrorAction SilentlyContinue).Property).Count -eq 0) {
        Remove-Item -Path $record -ErrorAction SilentlyContinue
    }
}
exit 0

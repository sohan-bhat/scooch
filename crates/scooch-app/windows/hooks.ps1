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

function Read-UserPath {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment')
    if (-not $key) { return }
    $value = [string]$key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    $key.Close()
    return @($value -split ';' | Where-Object { $_ })
}

function Write-UserPath([string[]]$Parts) {
    $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Environment')
    $key.SetValue('Path', ($Parts -join ';'), [Microsoft.Win32.RegistryValueKind]::ExpandString)
    $key.Close()
    [Environment]::SetEnvironmentVariable('SCOOCH_PATH_REFRESH', '1', 'User')
    [Environment]::SetEnvironmentVariable('SCOOCH_PATH_REFRESH', $null, 'User')
}

function Same([string]$A, [string]$B) {
    return $A.TrimEnd('\').ToLowerInvariant() -eq $B.TrimEnd('\').ToLowerInvariant()
}

function Path-Added {
    return [string](Get-ItemProperty -Path $record -Name PathAdded -ErrorAction SilentlyContinue).PathAdded
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
    $parts = @(Read-UserPath)
    if (-not ($parts | Where-Object { Same $_ $Folder })) {
        Write-UserPath (@($parts) + @($Folder))
        if (-not (Test-Path $record)) { New-Item -Path $record | Out-Null }
        Set-ItemProperty -Path $record -Name PathAdded -Value $Folder
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
    $off = Start-Process -FilePath $daemon -ArgumentList 'off' -WindowStyle Hidden -PassThru
    if (-not $off.WaitForExit(60000)) {
        Stop-Process -Id $off.Id -Force -ErrorAction SilentlyContinue
    }
}
$added = Path-Added
if ($added -and (Same $added $Folder)) {
    $parts = @(Read-UserPath)
    $kept = @($parts | Where-Object { -not (Same $_ $Folder) })
    if ($kept.Count -ne $parts.Count) {
        Write-UserPath $kept
    }
    Remove-ItemProperty -Path $record -Name PathAdded -ErrorAction SilentlyContinue
    if (@((Get-Item -Path $record -ErrorAction SilentlyContinue).Property).Count -eq 0) {
        Remove-Item -Path $record -ErrorAction SilentlyContinue
    }
}
exit 0

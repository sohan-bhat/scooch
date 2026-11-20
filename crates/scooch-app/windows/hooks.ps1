param(
    [Parameter(Mandatory = $true)][ValidateSet('installed', 'removing')][string]$Step,
    [Parameter(Mandatory = $true)][string]$Folder,
    [string]$WasRunning = 'no'
)

$ErrorActionPreference = 'Continue'
$Folder = $Folder.TrimEnd('\')
$marker = Join-Path $env:LOCALAPPDATA 'Scooch\reinstall.on'
$old = Join-Path $env:LOCALAPPDATA 'Programs\Scooch'

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

if ($Step -eq 'installed') {
    $parts = @(Read-UserPath)
    if (-not ($parts | Where-Object { Same $_ $Folder })) {
        Write-UserPath (@($parts) + @($Folder))
    }
    $daemon = Join-Path $Folder 'scoochw.exe'
    if ((Test-Path $marker) -and -not ($WasRunning -eq 'yes')) {
        Remove-Item -Force $marker -ErrorAction SilentlyContinue
        Start-Process -FilePath $daemon -WindowStyle Hidden
    } elseif ($WasRunning -eq 'yes') {
        Remove-Item -Force $marker -ErrorAction SilentlyContinue
        Start-Process -FilePath $daemon -ArgumentList 'watch' -WindowStyle Hidden
    }
    exit 0
}

$daemon = Join-Path $Folder 'scoochw.exe'
$running = @(Get-Process -Name 'scoochw' -ErrorAction SilentlyContinue)
if ($running.Count -gt 0) {
    New-Item -ItemType Directory -Force -Path (Split-Path $marker) | Out-Null
    Set-Content -Path $marker -Value 'on'
}
if (Test-Path $daemon) {
    $off = Start-Process -FilePath $daemon -ArgumentList 'off' -WindowStyle Hidden -PassThru
    if (-not $off.WaitForExit(60000)) {
        Stop-Process -Id $off.Id -Force -ErrorAction SilentlyContinue
    }
}
$parts = @(Read-UserPath)
$kept = @($parts | Where-Object { -not (Same $_ $Folder) })
if ($kept.Count -ne $parts.Count) {
    Write-UserPath $kept
}
exit 0

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$repo = 'sohan-bhat/scooch'
$base = if ($env:SCOOCH_RELEASE_URL) { $env:SCOOCH_RELEASE_URL } else { "https://github.com/$repo/releases/latest/download" }
$arm = ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') -or ($env:PROCESSOR_ARCHITEW6432 -eq 'ARM64')
$arch = if ($arm) { 'aarch64' } else { 'x86_64' }
$asset = "scooch-windows-$arch.zip"
$dir = if ($env:SCOOCH_INSTALL) { $env:SCOOCH_INSTALL } else { Join-Path $env:LOCALAPPDATA 'Programs\Scooch' }
$tmp = Join-Path ([IO.Path]::GetTempPath()) ('scooch-' + [Guid]::NewGuid().ToString('N'))

[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

New-Item -ItemType Directory -Force -Path $tmp | Out-Null
New-Item -ItemType Directory -Force -Path $dir | Out-Null

try {
    $zip = Join-Path $tmp $asset
    try {
        Invoke-WebRequest -UseBasicParsing -Uri "$base/$asset" -OutFile $zip
    } catch {
        throw 'The download failed, so check your connection and run this again.'
    }

    $sums = $null
    try {
        $sumsFile = Join-Path $tmp 'SHA256SUMS'
        Invoke-WebRequest -UseBasicParsing -Uri "$base/SHA256SUMS" -OutFile $sumsFile
        $sums = Get-Content $sumsFile
    } catch {
        $sums = $null
    }
    if ($sums) {
        $line = $sums | Where-Object { $_ -match (' ' + [Regex]::Escape($asset) + '$') } | Select-Object -First 1
        if ($line) {
            $want = ($line -split '\s+')[0].ToLower()
            $got = (Get-FileHash -Algorithm SHA256 -Path $zip).Hash.ToLower()
            if ($want -ne $got) {
                throw 'The download did not match its checksum, so nothing was installed. Run this again.'
            }
        }
    }

    Expand-Archive -Force -Path $zip -DestinationPath $tmp
    if (-not (Test-Path (Join-Path $tmp 'scooch.exe'))) {
        throw 'The download was not a Scooch build, so nothing was installed.'
    }

    $running = @(Get-Process -Name 'scoochw' -ErrorAction SilentlyContinue | Where-Object { $_.Path -and ((Split-Path $_.Path) -eq $dir) })
    foreach ($process in $running) {
        Stop-Process -Id $process.Id -Force
        $process.WaitForExit(5000) | Out-Null
    }

    foreach ($name in 'scooch.exe', 'scoochw.exe') {
        $from = Join-Path $tmp $name
        if (Test-Path $from) {
            Copy-Item -Force -Path $from -Destination (Join-Path $dir $name)
        }
    }

    if ($running.Count -gt 0) {
        Start-Process -FilePath (Join-Path $dir 'scoochw.exe') -ArgumentList 'watch' -WindowStyle Hidden
    }
} finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}

$key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
$old = [string]$key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
$parts = @($old -split ';' | Where-Object { $_ })
if ($parts -notcontains $dir) {
    $key.SetValue('Path', (($parts + $dir) -join ';'), [Microsoft.Win32.RegistryValueKind]::ExpandString)
    [Environment]::SetEnvironmentVariable('SCOOCH_PATH_REFRESH', $null, 'User')
}
$key.Close()

if (($env:Path -split ';') -notcontains $dir) {
    $env:Path = "$env:Path;$dir"
}

Write-Host 'Scooch is installed. Run scooch to turn it on.'

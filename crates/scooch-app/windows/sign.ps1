param([Parameter(Mandatory = $true)][string]$File)

if (-not $env:WINDOWS_CERTIFICATE -or -not $env:WINDOWS_CERTIFICATE_PASSWORD) {
    exit 0
}

$pfx = Join-Path ([IO.Path]::GetTempPath()) 'scooch-sign.pfx'
try {
    [IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:WINDOWS_CERTIFICATE))
    $signtool = Get-ChildItem -Path "${env:ProgramFiles(x86)}\Windows Kits\10\bin" -Recurse -Filter signtool.exe -ErrorAction SilentlyContinue | Where-Object { $_.FullName -match '\\x64\\' } | Sort-Object FullName -Descending | Select-Object -First 1
    if (-not $signtool) {
        Write-Error 'signtool.exe was not found.'
        exit 1
    }
    & $signtool.FullName sign /fd SHA256 /td SHA256 /tr 'http://timestamp.digicert.com' /f $pfx /p $env:WINDOWS_CERTIFICATE_PASSWORD $File
    exit $LASTEXITCODE
} finally {
    Remove-Item -Force $pfx -ErrorAction SilentlyContinue
}

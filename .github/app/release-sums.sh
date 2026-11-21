#!/usr/bin/env bash
set -eu

cd "$1"
rm -f SHA256SUMS
sha256sum -- * >"$RUNNER_TEMP/SHA256SUMS"
mv "$RUNNER_TEMP/SHA256SUMS" SHA256SUMS
if grep -q ' SHA256SUMS$' SHA256SUMS; then
  echo "The sums list themselves."
  exit 1
fi
sha256sum --check --strict SHA256SUMS
for asset in scooch-macos.tar.gz scooch-linux-x86_64.tar.gz scooch-linux-aarch64.tar.gz; do
  want=$(grep " $asset\$" SHA256SUMS | cut -d' ' -f1 || true)
  got=$(sha256sum "$asset" | cut -d' ' -f1)
  if [ -z "$want" ] || [ "$want" != "$got" ]; then
    echo "install.sh would not find a matching line for $asset."
    exit 1
  fi
  echo "install.sh finds $asset"
done
for asset in scooch-windows-x86_64.zip scooch-windows-aarch64.zip; do
  pwsh -NoProfile -Command "\$sums = Get-Content SHA256SUMS; \$line = \$sums | Where-Object { \$_ -match (' ' + [Regex]::Escape('$asset') + '\$') } | Select-Object -First 1; \$want = (\$line -split '\\s+')[0].ToLower(); \$got = (Get-FileHash -Algorithm SHA256 -Path '$asset').Hash.ToLower(); if (-not \$line -or \$want -ne \$got) { Write-Error 'install.ps1 would not find a matching line for $asset'; exit 1 }; 'install.ps1 finds $asset'"
done
cat SHA256SUMS

want="SHA256SUMS Scooch-Setup-arm64.exe Scooch-Setup-x64.exe Scooch-aarch64.AppImage Scooch-x86_64.AppImage Scooch.dmg scooch-linux-aarch64.tar.gz scooch-linux-x86_64.tar.gz scooch-macos.tar.gz scooch-windows-aarch64.zip scooch-windows-x86_64.zip scooch.aarch64.rpm scooch.x86_64.rpm scooch_amd64.deb scooch_arm64.deb"
if [ -f latest.json ]; then
  want="$want latest.json Scooch.app.tar.gz Scooch.app.tar.gz.sig Scooch-Setup-arm64.exe.sig Scooch-Setup-x64.exe.sig Scooch-aarch64.AppImage.sig Scooch-x86_64.AppImage.sig scooch.aarch64.rpm.sig scooch.x86_64.rpm.sig scooch_amd64.deb.sig scooch_arm64.deb.sig"
fi
have=$(LC_ALL=C ls -1 | LC_ALL=C sort | tr '\n' ' ' | sed 's/ $//')
want=$(printf '%s\n' $want | LC_ALL=C sort | tr '\n' ' ' | sed 's/ $//')
if [ "$have" != "$want" ]; then
  echo "The release would attach $have"
  echo "and should attach $want"
  exit 1
fi
echo "The release attaches these $(ls -1 | wc -l) files"
ls -l

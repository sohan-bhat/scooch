#!/usr/bin/env bash
set -eu

here="$(cd "$(dirname "$0")" && pwd)"
ruby "$here/guard.rb" .github

plant() {
  local label="$1" body="$2" copy
  copy=$(mktemp -d)
  cp -R .github "$copy/"
  printf '%s\n' "$body" >>"$copy/.github/workflows/app.yml"
  if ruby "$here/guard.rb" "$copy/.github" >"$copy/out.txt" 2>&1; then
    echo "The guard passed a planted $label."
    exit 1
  fi
  echo "The guard stopped a planted $label: $(head -1 "$copy/out.txt")"
}

open='${{'
close='}}'
plant "secrets.NAME" "
  planted:
    runs-on: ubuntu-latest
    steps:
      - name: Probe
        env:
          PLANTED: $open secrets.PLANTED $close
        run: 'true'"
plant "secrets['NAME']" "
  planted:
    runs-on: ubuntu-latest
    steps:
      - name: Probe
        env:
          PLANTED: $open secrets['PLANTED'] $close
        run: 'true'"
plant "toJSON(secrets)" "
  planted:
    runs-on: ubuntu-latest
    steps:
      - name: Probe
        run: echo '$open toJSON(secrets) $close'"
plant "secrets: inherit" "
  planted:
    uses: ./.github/workflows/install-check.yml
    secrets: inherit"
plant "secrets map of a called workflow" "
  planted:
    uses: ./.github/workflows/install-check.yml
    secrets:
      PLANTED: $open secrets.PLANTED $close"
plant "services container env" "
  planted:
    runs-on: ubuntu-latest
    services:
      store:
        image: busybox
        env:
          PLANTED: $open secrets.PLANTED $close
    steps:
      - run: 'true'"
plant "GITHUB_TOKEN" "
  planted:
    runs-on: ubuntu-latest
    steps:
      - env:
          GH_TOKEN: $open secrets.GITHUB_TOKEN $close
        run: 'true'"

copy=$(mktemp -d)
cp -R .github "$copy/"
ruby -ryaml -e '
  path = ARGV[0]
  flow = YAML.safe_load(File.read(path), aliases: true)
  flow["jobs"]["bundle-macos"]["steps"].insert(2, { "name" => "Planted build", "run" => "cargo build --release" })
  File.write(path, YAML.dump(flow))
' "$copy/.github/workflows/app.yml"
if ruby "$here/guard.rb" "$copy/.github" >"$copy/out.txt" 2>&1; then
  echo "The guard passed a cargo build planted in a job that holds a secret."
  exit 1
fi
echo "The guard stopped a cargo build planted in a job that holds a secret: $(grep -m1 'Planted build' "$copy/out.txt")"

work=$(mktemp -d)
if bash -c 'line() { echo "$*"; }; . "$1"; expect "a planted false expectation" false; finish' sh "$here/expect.sh" >"$work/bash.txt" 2>&1; then
  echo "The bash expectation helper passed a false expectation."
  exit 1
fi
grep -q "Expectations 1, failed 1." "$work/bash.txt" || { echo "The bash expectation helper did not count the planted failure."; exit 1; }
echo "The bash expectation helper failed a false expectation: $(tail -1 "$work/bash.txt")"
if bash -c 'line() { echo "$*"; }; . "$1"; finish' sh "$here/expect.sh" >"$work/none.txt" 2>&1; then
  echo "The bash expectation helper passed a probe with no expectation."
  exit 1
fi
if ! command -v pwsh >/dev/null; then
  echo "PowerShell is missing here, so its helper is checked on the runner only."
  exit 0
fi
if pwsh -NoProfile -Command "function Line([string]\$Text) { Write-Output \$Text }; . '$here/expect.ps1'; Expect 'a planted false expectation' \$false; Finish" >"$work/pwsh.txt" 2>&1; then
  echo "The PowerShell expectation helper passed a false expectation."
  exit 1
fi
echo "The PowerShell expectation helper failed a false expectation: $(tail -1 "$work/pwsh.txt")"
grep -q "Expectations 1, failed 1." "$work/pwsh.txt" || { echo "The PowerShell expectation helper did not count the planted failure."; exit 1; }

#!/usr/bin/env bash
set -eu

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
cd "$root"
ruby "$here/guard.rb" "$root"

while read -r kind repo sha tag; do
  [ "$kind" = action ] || continue
  if ! [[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "The lock ties $repo to $tag, which is not a release tag."
    exit 1
  fi
  got=$(git ls-remote "https://github.com/$repo" "refs/tags/$tag" "refs/tags/$tag^{}" | awk -v t="refs/tags/$tag" '$2 == t { plain = $1 } $2 == t "^{}" { peeled = $1 } END { print (peeled != "" ? peeled : plain) }')
  if [ "$got" != "$sha" ]; then
    echo "The release tag $tag of $repo is the commit ${got:-nothing}, the lock says $sha."
    exit 1
  fi
  echo "The release tag $tag of $repo is the commit $sha."
done <"$here/guard.lock"

tree() {
  local copy
  copy=$(mktemp -d)
  tar -C "$root" --exclude=target --exclude=node_modules -cf - .github crates/scooch-app | tar -C "$copy" -xf -
  echo "$copy"
}

judge() {
  local label="$1" expect="$2" copy="$3"
  if ruby "$copy/.github/app/guard.rb" "$copy" >"$copy/out.txt" 2>&1; then
    echo "The guard passed a planted $label."
    exit 1
  fi
  if ! grep -q -F -- "$expect" "$copy/out.txt"; then
    echo "The guard stopped a planted $label, but not with \"$expect\". It said:"
    cat "$copy/out.txt"
    exit 1
  fi
  echo "The guard stopped a planted $label: $(grep -m1 -F -- "$expect" "$copy/out.txt")"
  rm -rf "$copy"
}

plant() {
  local label="$1" expect="$2" body="$3" copy
  copy=$(tree)
  printf '%s\n' "$body" >>"$copy/.github/workflows/app.yml"
  judge "$label" "$expect" "$copy"
}

edit() {
  local label="$1" expect="$2" file="$3" code="$4" copy
  copy=$(tree)
  ruby -ryaml -rjson -e '
    path, code = ARGV
    json = path.end_with?(".json")
    flow = json ? JSON.parse(File.read(path)) : YAML.safe_load(File.read(path), aliases: true)
    eval(code)
    File.write(path, json ? JSON.pretty_generate(flow) : YAML.dump(flow))
  ' "$copy/$file" "$code"
  judge "$label" "$expect" "$copy"
}

touched() {
  local label="$1" expect="$2" command="$3" copy
  copy=$(tree)
  (cd "$copy" && eval "$command")
  judge "$label" "$expect" "$copy"
}

open='${{'
close='}}'
plant "secrets.NAME" "names secrets" "
  planted:
    runs-on: ubuntu-latest
    steps:
      - name: Probe
        env:
          PLANTED: $open secrets.PLANTED $close
        run: 'true'"
plant "secrets['NAME']" "names secrets" "
  planted:
    runs-on: ubuntu-latest
    steps:
      - name: Probe
        env:
          PLANTED: $open secrets['PLANTED'] $close
        run: 'true'"
plant "toJSON(secrets)" "names secrets" "
  planted:
    runs-on: ubuntu-latest
    steps:
      - name: Probe
        run: echo '$open toJSON(secrets) $close'"
plant "secrets: inherit" "names secrets" "
  planted:
    uses: ./.github/workflows/install-check.yml
    secrets: inherit"
plant "secrets map of a called workflow" "names secrets" "
  planted:
    uses: ./.github/workflows/install-check.yml
    secrets:
      PLANTED: $open secrets.PLANTED $close"
plant "services container env" "names secrets" "
  planted:
    runs-on: ubuntu-latest
    services:
      store:
        image: busybox
        env:
          PLANTED: $open secrets.PLANTED $close
    steps:
      - run: 'true'"
plant "GITHUB_TOKEN" "names secrets" "
  planted:
    runs-on: ubuntu-latest
    steps:
      - env:
          GH_TOKEN: $open secrets.GITHUB_TOKEN $close
        run: 'true'"
plant "github.token in a job that is not listed" "names secrets or the token" "
  planted:
    runs-on: ubuntu-latest
    steps:
      - env:
          GH_TOKEN: $open github.token $close
        run: 'true'"
plant "toJSON(github)" "names secrets or the token" "
  planted:
    runs-on: ubuntu-latest
    steps:
      - run: echo '$open toJSON(github) $close'"

edit "cargo build in a job that holds a secret" "is not one call of a listed script" .github/workflows/app.yml \
  'flow["jobs"]["bundle-macos"]["steps"].insert(2, { "name" => "Planted build", "run" => "cargo build --release" })'
edit "shell on a secret step" "has shell" .github/workflows/app.yml \
  'flow["jobs"]["bundle-macos"]["steps"].find { |s| s["name"] == "Bundle and sign" }["shell"] = "env BASH_ENV=/tmp/planted bash {0}"'
edit "NODE_OPTIONS on a secret step" "sets env NODE_OPTIONS" .github/workflows/app.yml \
  'flow["jobs"]["bundle-macos"]["steps"].find { |s| s["name"] == "Bundle and sign" }["env"]["NODE_OPTIONS"] = "--require /tmp/planted.js"'
edit "TAURI_CONFIG on a secret step" "sets env TAURI_CONFIG" .github/workflows/app.yml \
  'flow["jobs"]["bundle-windows"]["steps"].find { |s| s["name"] == "Bundle and sign" }["env"]["TAURI_CONFIG"] = "{}"'
edit "download into the checkout in a secret job" "downloads into the checkout" .github/workflows/app.yml \
  'flow["jobs"]["bundle-macos"]["steps"].find { |s| s["uses"].to_s.start_with?("actions/download-artifact") }["with"]["path"] = ".github/app"'
edit "matrix value with a command" "matrix value that is not a plain word" .github/workflows/app.yml \
  'flow["jobs"]["bundle-windows"]["strategy"]["matrix"]["include"][0]["target"] = "x86_64-pc-windows-msvc; curl planted | sh"'
edit "defaults.run.shell at the top" "has defaults at its top" .github/workflows/app.yml \
  'flow["defaults"] = { "run" => { "shell" => "env BASH_ENV=/tmp/planted bash {0}" } }'
edit "self-hosted runner for a secret job" "not a GitHub hosted label" .github/workflows/app.yml \
  'flow["jobs"]["bundle-macos"]["runs-on"] = "self-hosted"'
edit "write permission on a build job" "has a write permission" .github/workflows/app.yml \
  'flow["jobs"]["build-macos"]["permissions"] = { "contents" => "write" }'
edit "write permission at the top" "grants more than contents read" .github/workflows/app.yml \
  'flow["permissions"] = { "contents" => "write" }'
edit "checkout that keeps its credentials" "keeps its credentials" .github/workflows/app.yml \
  'flow["jobs"]["build-macos"]["steps"][0].delete("with")'
edit "action by a moving tag" "not pinned by a commit" .github/workflows/app.yml \
  'flow["jobs"]["build-macos"]["steps"][0]["uses"] = "actions/checkout@v4"'
edit "pull_request_target trigger" "starts on pull_request_target" .github/workflows/app.yml \
  'on = flow.key?("on") ? "on" : true; flow[on]["pull_request_target"] = nil'
edit "changed job that holds a secret" "guard.lock: job app.yml:updater-sign is now" .github/workflows/app.yml \
  'flow["jobs"]["updater-sign"]["timeout-minutes"] = 21'
edit "beforeBundleCommand" "beforeBundleCommand" crates/scooch-app/tauri.conf.json \
  'flow["build"]["beforeBundleCommand"] = "sh planted.sh"'
edit "another signCommand" "signCommand" crates/scooch-app/tauri.windows.conf.json \
  'flow["bundle"]["windows"]["signCommand"] = "pwsh -NoProfile -File planted.ps1 %1"'
edit "other NSIS hooks" "installerHooks" crates/scooch-app/tauri.windows.conf.json \
  'flow["bundle"]["windows"]["nsis"]["installerHooks"] = "windows/planted.nsh"'
touched "changed byte in a frozen script" "guard.lock: file .github/app/take.sh is now" 'printf "\n" >>.github/app/take.sh'
touched "changed byte in sign.ps1" "guard.lock: file crates/scooch-app/windows/sign.ps1 is now" 'printf " " >>crates/scooch-app/windows/sign.ps1'
touched "Tauri config in TOML" "keep the config in JSON" 'printf "[build]\n" >crates/scooch-app/Tauri.toml'

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
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

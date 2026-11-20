#!/usr/bin/env bash
set -eu

ruby .github/app/secrets-check.rb .github
leak=$(mktemp -d)
cp -R .github "$leak/"
cat >>"$leak/.github/workflows/app.yml" <<'PLANT'

  planted:
    runs-on: ubuntu-latest
    steps:
      - name: Probe
        env:
          APPLE_ID: ${{ secrets.APPLE_ID }}
        run: "true"
PLANT
if ruby .github/app/secrets-check.rb "$leak/.github"; then
  echo "The check passed a planted leak."
  exit 1
fi
echo "The check stopped the planted leak above."

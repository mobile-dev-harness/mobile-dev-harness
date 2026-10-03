#!/bin/sh
# Replaces the vendored compatibility knowledge base with a release of compat-kb, checking its
# SHA-256. Usage: scripts/update-kb.sh v1.1.0
set -eu
version=${1:?usage: scripts/update-kb.sh <version, e.g. v1.1.0>}
cd "$(dirname "$0")/../crates/mdh-risk/kb"
base=https://github.com/mobile-dev-harness/compat-kb/releases/download/$version
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
curl -fsSL -o "$tmp/android.yaml" "$base/android.yaml"
curl -fsSL -o "$tmp/android.yaml.sha256" "$base/android.yaml.sha256"
expected=$(cut -d' ' -f1 "$tmp/android.yaml.sha256")
actual=$(shasum -a 256 "$tmp/android.yaml" | cut -d' ' -f1)
if [ "$expected" != "$actual" ]; then
  echo "checksum mismatch: release says $expected, the file is $actual" >&2
  exit 1
fi
mv "$tmp/android.yaml" android.yaml
sed -i.bak -e "s/^version = .*/version = $version/" -e "s/^sha256 = .*/sha256 = $actual/" SOURCE
rm -f SOURCE.bak
echo "knowledge base $version ($actual); run cargo test -p mdh-compat, then commit"

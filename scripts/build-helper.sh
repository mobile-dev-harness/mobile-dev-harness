#!/usr/bin/env bash
# Rebuilds the on-device helper APK and copies it to where mdh-driver embeds it from.
# Run after changing anything under android-helper/, and bump the helper version code first
# (android-helper/build.gradle.kts, Commands.VERSION_CODE, HELPER_VERSION_CODE in helper.rs).
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
sdk="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/Library/Android/sdk}}"

cd "$root/android-helper"
echo "sdk.dir=$sdk" > local.properties
./gradlew --quiet assembleRelease
cp build/outputs/apk/release/mdh-helper-release.apk "$root/crates/mdh-driver/assets/mdh-helper.apk"
echo "updated crates/mdh-driver/assets/mdh-helper.apk ($(wc -c < "$root/crates/mdh-driver/assets/mdh-helper.apk") bytes)"

#!/usr/bin/env bash
# Builds the Android app: crates/app as libcpz_app.so, then the APK around it
# in android/.
#
#   scripts/android.sh              # release APK for phones (arm64) → dist/
#   scripts/android.sh install      # that, then install and start it over adb
#   scripts/android.sh emulator     # debug APK for an x86_64 emulator, installed
#   scripts/android.sh logs         # follow the app's log over adb
#
# Needs the Android SDK (ANDROID_HOME, default ~/Android/Sdk) with an NDK, a
# JDK 17 or 21 (JAVA_HOME, default /usr/lib/jvm/java-21-openjdk), cargo-ndk,
# and the rustup targets aarch64-linux-android / x86_64-linux-android.
#
# Release builds are signed with the key named in android/keystore.properties.
# The first release build makes one in ~/.config/cpz-music/ if there is none: back it
# up, because installed copies only accept updates signed with the same key.
set -euo pipefail
cd "$(dirname "$0")/.."

export ANDROID_HOME="${ANDROID_HOME:-$HOME/Android/Sdk}"
export JAVA_HOME="${JAVA_HOME:-/usr/lib/jvm/java-21-openjdk}"
# Slint's build script compiles a small Java helper against this.
platform=$(ls -d "$ANDROID_HOME"/platforms/android-* | sort -V | tail -1)
export ANDROID_JAR="${ANDROID_JAR:-$platform/android.jar}"
APP_ID=uk.capzay.music

version=$(sed -n 's/^version = "\(.*\)"/\1/p' crates/app/Cargo.toml | head -1)

rust() { # rust <abi> <release|debug>
    local flags=()
    [ "$2" = release ] && flags=(--release)
    rm -rf android/app/src/main/jniLibs
    cargo ndk -t "$1" -P 29 -o android/app/src/main/jniLibs \
        rustc -p cpz-app --lib --crate-type cdylib "${flags[@]}"
}

keystore() {
    [ -f android/keystore.properties ] && return
    local file="$HOME/.config/cpz-music/android-release.jks"
    local password
    if [ ! -f "$file" ]; then
        password=$(head -c 24 /dev/urandom | base64 | tr -d '/+=')
        mkdir -p "$HOME/.config/cpz-music"
        "$JAVA_HOME/bin/keytool" -genkeypair -keystore "$file" -alias cpz -keyalg RSA \
            -keysize 4096 -validity 36500 -storepass "$password" -keypass "$password" \
            -dname "CN=Cpz Music" >/dev/null
        printf 'storePassword=%s\n' "$password" > "$HOME/.config/cpz-music/android-release.password"
        chmod 600 "$file" "$HOME/.config/cpz-music/android-release.password"
        echo "made a signing key: $file (back it up with its password file)"
    fi
    password=$(sed -n 's/^storePassword=//p' "$HOME/.config/cpz-music/android-release.password")
    printf 'storeFile=%s\nstorePassword=%s\nkeyAlias=cpz\nkeyPassword=%s\n' \
        "$file" "$password" "$password" > android/keystore.properties
    chmod 600 android/keystore.properties
}

gradle() {
    printf 'sdk.dir=%s\n' "$ANDROID_HOME" > android/local.properties
    (cd android && ./gradlew --quiet "$@")
}

release() {
    keystore
    rust arm64-v8a release
    gradle assembleRelease
    mkdir -p dist
    cp android/app/build/outputs/apk/release/app-release.apk "dist/cpz-music-$version.apk"
    echo "dist/cpz-music-$version.apk"
}

start() {
    adb install -r "$1"
    adb shell am start -n "$APP_ID/.MainActivity" >/dev/null
}

case "${1:-release}" in
    release) release ;;
    install) release && start "dist/cpz-music-$version.apk" ;;
    emulator)
        rust x86_64 debug
        gradle assembleDebug
        start android/app/build/outputs/apk/debug/app-debug.apk
        ;;
    logs) adb logcat -v brief -s cpz-music:V RustStdoutStderr:V AndroidRuntime:E ;;
    *) sed -n '2,10p' "$0"; exit 1 ;;
esac

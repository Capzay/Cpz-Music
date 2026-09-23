#!/usr/bin/env bash
# One-shot Android APK build / install for the Capacitor shell.
#
# Usage:
#   ./scripts/apk.sh              # sync + debug APK → dist/
#   ./scripts/apk.sh install      # debug APK + adb install + launch
#   ./scripts/apk.sh release      # sync + release APK → dist/
#   ./scripts/apk.sh doctor       # check Java / SDK / adb, write local.properties
#
# Env:
#   CPZ_SERVER_URL   baked into the APK at sync time (default: https://music.capzay.uk)
#   CPZ_WEB_DEBUG=1  enable Chrome WebView debugging in the baked config
#   ANDROID_HOME     or ANDROID_SDK_ROOT — Android SDK path
#   JAVA_HOME        JDK used by Gradle

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ANDROID_DIR="$ROOT/android"
DIST_DIR="$ROOT/dist"
APP_ID="uk.capzay.music"
MAIN_ACTIVITY=".MainActivity"

red()  { printf '\033[31m%s\033[0m\n' "$*"; }
bold() { printf '\033[1m%s\033[0m\n' "$*"; }
ok()   { printf '  ✓ %s\n' "$*"; }
fail() { red "  ✗ $*"; return 1; }

die() {
  red "error: $*" >&2
  exit 1
}

cmd="${1:-debug}"

find_sdk() {
  local candidates=(
    "${ANDROID_HOME:-}"
    "${ANDROID_SDK_ROOT:-}"
    "$HOME/Android/Sdk"
    /opt/android-sdk
    /usr/lib/android-sdk
  )
  local c
  for c in "${candidates[@]}"; do
    [[ -n "$c" && -d "$c" ]] || continue
    if [[ -d "$c/platforms" || -d "$c/build-tools" || -f "$c/cmdline-tools/latest/bin/sdkmanager" ]]; then
      printf '%s' "$c"
      return 0
    fi
  done
  return 1
}

# Returns a Gradle-compatible JDK (17–21 preferred). Java 26+ breaks this
# project's Gradle 8.14 ("Unsupported class file major version 70").
java_major() {
  local home="$1"
  "$home/bin/java" -XshowSettings:properties -version 2>&1 \
    | awk -F'= ' '/java.specification.version/ {
        v=$2; sub(/^1\./,"",v); print int(v); exit
      }'
}

find_java_home() {
  local candidates=()
  local c home major

  # Explicit override only if it is new enough for AGP and old enough for Gradle.
  if [[ -n "${JAVA_HOME:-}" && -x "${JAVA_HOME}/bin/java" ]]; then
    candidates+=("$JAVA_HOME")
  fi
  candidates+=(
    /usr/lib/jvm/java-21-openjdk
    /usr/lib/jvm/java-17-openjdk
    /usr/lib/jvm/default
  )
  if command -v java >/dev/null 2>&1; then
    home="$(java -XshowSettings:properties -version 2>&1 | awk -F'= ' '/java.home/ {print $2; exit}')"
    [[ -n "$home" ]] && candidates+=("$home" "${home%/jre}")
  fi

  local fallback=""
  for c in "${candidates[@]}"; do
    [[ -n "$c" && -x "$c/bin/java" ]] || continue
    major="$(java_major "$c" || true)"
    [[ -n "$major" ]] || continue
    # Gradle 8.14 supports up through Java 24; AGP is happiest on 17/21.
    if (( major >= 17 && major <= 21 )); then
      printf '%s' "$c"
      return 0
    fi
    if (( major >= 17 && major <= 24 )) && [[ -z "$fallback" ]]; then
      fallback="$c"
    fi
  done
  if [[ -n "$fallback" ]]; then
    printf '%s' "$fallback"
    return 0
  fi
  return 1
}

ensure_local_properties() {
  local sdk="$1"
  local props="$ANDROID_DIR/local.properties"
  if [[ -f "$props" ]] && grep -qF "sdk.dir=$sdk" "$props" 2>/dev/null; then
    return 0
  fi
  printf 'sdk.dir=%s\n' "$sdk" >"$props"
  ok "wrote $props"
}

ensure_deps() {
  if [[ ! -d "$ROOT/node_modules/@capacitor/cli" ]]; then
    bold "Installing mobile dependencies…"
    (cd "$ROOT" && npm install)
  fi
}

doctor() {
  local status=0
  bold "Cpz Music Android toolchain"

  if [[ -d "$ROOT/node_modules/@capacitor/cli" ]]; then
    ok "node_modules present"
  else
    fail "run: cd mobile && npm install" || status=1
  fi

  local java_home=""
  if java_home="$(find_java_home)"; then
    export JAVA_HOME="$java_home"
    ok "Java: $("$JAVA_HOME/bin/java" -version 2>&1 | head -1) ($JAVA_HOME)"
  else
    fail "JDK 17 or 21 not found (Gradle cannot use Java 26+). Install e.g. jdk21-openjdk." || status=1
  fi

  local sdk=""
  if sdk="$(find_sdk)"; then
    export ANDROID_HOME="$sdk"
    export ANDROID_SDK_ROOT="$sdk"
    ok "Android SDK: $sdk"
    ensure_local_properties "$sdk"
  else
    fail "Android SDK not found. Install Android Studio or cmdline-tools, then set ANDROID_HOME." || status=1
  fi

  if command -v adb >/dev/null 2>&1; then
    ok "adb: $(command -v adb)"
    local devices
    devices="$(adb devices 2>/dev/null | awk 'NR>1 && $2=="device" {print $1}')"
    if [[ -n "$devices" ]]; then
      ok "device(s): $(printf '%s' "$devices" | tr '\n' ' ')"
    else
      printf '  · no device/emulator online (needed for install)\n'
    fi
  else
    printf '  · adb not on PATH (optional; needed for npm run apk:install)\n'
  fi

  if [[ -x "$ANDROID_DIR/gradlew" ]]; then
    ok "gradlew present"
  else
    fail "missing $ANDROID_DIR/gradlew" || status=1
  fi

  printf '\nServer URL (baked at sync): %s\n' "${CPZ_SERVER_URL:-https://music.capzay.uk}"
  return "$status"
}

sync_cap() {
  ensure_deps
  bold "Syncing Capacitor (server.url=${CPZ_SERVER_URL:-https://music.capzay.uk})…"
  (cd "$ROOT" && npx cap sync android)
}

build_apk() {
  local variant="$1" # debug | release
  local task apk_src apk_name

  doctor || die "fix the toolchain issues above, then retry."

  sync_cap

  if [[ "$variant" == "release" ]]; then
    task="assembleRelease"
    apk_name="cpz-music-release.apk"
  else
    task="assembleDebug"
    apk_name="cpz-music-debug.apk"
  fi

  bold "Building $variant APK…"
  (cd "$ANDROID_DIR" && ./gradlew --quiet "$task")

  # Prefer the conventional name; fall back to whatever AGP emitted
  # (e.g. app-release-unsigned.apk when no keystore is configured).
  apk_src="$ANDROID_DIR/app/build/outputs/apk/$variant/app-$variant.apk"
  if [[ ! -f "$apk_src" ]]; then
    apk_src="$(find "$ANDROID_DIR/app/build/outputs/apk/$variant" -name '*.apk' -type f 2>/dev/null | head -1 || true)"
  fi
  [[ -n "$apk_src" && -f "$apk_src" ]] || die "Gradle finished but no APK under app/build/outputs/apk/$variant"

  mkdir -p "$DIST_DIR"
  cp -f "$apk_src" "$DIST_DIR/$apk_name"
  bold "APK ready: $DIST_DIR/$apk_name"
  ls -lh "$DIST_DIR/$apk_name"
}

install_apk() {
  build_apk debug
  command -v adb >/dev/null 2>&1 || die "adb not found — install android-tools / platform-tools"
  adb devices | awk 'NR>1 && $2=="device" {found=1} END {exit !found}' \
    || die "no device/emulator online (enable USB debugging or start an emulator)"

  bold "Installing…"
  adb install -r "$DIST_DIR/cpz-music-debug.apk"
  bold "Launching $APP_ID…"
  adb shell am start -n "$APP_ID/$MAIN_ACTIVITY" >/dev/null
  ok "installed and launched"
}

case "$cmd" in
  debug|build|"")
    build_apk debug
    ;;
  release)
    build_apk release
    ;;
  install|run)
    install_apk
    ;;
  doctor)
    doctor
    ;;
  -h|--help|help)
    cat <<'EOF'
One-shot Android APK build / install for the Capacitor shell.

Usage:
  ./scripts/apk.sh              # sync + debug APK → dist/
  ./scripts/apk.sh install      # debug APK + adb install + launch
  ./scripts/apk.sh release      # sync + release APK → dist/
  ./scripts/apk.sh doctor       # check Java / SDK / adb, write local.properties

Env:
  CPZ_SERVER_URL   baked into the APK at sync time (default: https://music.capzay.uk)
  CPZ_WEB_DEBUG=1  enable Chrome WebView debugging in the baked config
  ANDROID_HOME     or ANDROID_SDK_ROOT — Android SDK path
  JAVA_HOME        JDK used by Gradle
EOF
    ;;
  *)
    die "unknown command: $cmd (try: debug | release | install | doctor)"
    ;;
esac

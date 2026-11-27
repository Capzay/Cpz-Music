#!/usr/bin/env bash
# Builds the app and puts it on the server, so every running copy offers to
# update. Bump `version` in crates/app/Cargo.toml first: the app only updates
# to a version higher than its own.
#
#   scripts/publish-client.sh                      # Linux x86_64
#   scripts/publish-client.sh --windows            # Windows x86_64 too (needs mingw-w64-gcc)
#   scripts/publish-client.sh --android            # the Android APK too (see crates/xtask)
#   scripts/publish-client.sh --android --no-linux # only Android
#
# HOST and DIR say where the server keeps builds (its RELEASES_DIR).
set -euo pipefail

HOST="${HOST:-capzaypi}"
DIR="${DIR:-Cpz-Music/releases}"
cd "$(dirname "$0")/.."

version=$(sed -n 's/^version = "\(.*\)"/\1/p' crates/app/Cargo.toml | head -1)
[ -n "$version" ] || { echo "no version in crates/app/Cargo.toml" >&2; exit 1; }

publish() {
    local platform=$1 binary=$2 file=$3
    local current
    current=$(ssh "$HOST" "cat $DIR/$platform/latest.json 2>/dev/null" | sed -n 's/.*"version": *"\([^"]*\)".*/\1/p')
    if [ "$current" = "$version" ]; then
        echo "$platform already has $version: bump the version in crates/app/Cargo.toml" >&2
        exit 1
    fi
    local sha
    sha=$(sha256sum "$binary" | cut -d' ' -f1)
    local name="$file-$version"
    [[ "$file" == *.exe ]] && name="${file%.exe}-$version.exe"
    [[ "$file" == *.apk ]] && name="${file%.apk}-$version.apk"
    echo "publishing $platform $version (was ${current:-none})"
    ssh "$HOST" "mkdir -p $DIR/$platform"
    scp -q "$binary" "$HOST:$DIR/$platform/$name"
    # The manifest goes last and is renamed into place, so the server never
    # points at a half-copied build.
    printf '{ "version": "%s", "file": "%s", "sha256": "%s" }\n' "$version" "$name" "$sha" |
        ssh "$HOST" "cat > $DIR/$platform/latest.json.new && mv $DIR/$platform/latest.json.new $DIR/$platform/latest.json && find $DIR/$platform -maxdepth 1 -name '${file%.*}-*' ! -name '$name' -delete"
}

linux=1 windows=0 android=0
for arg in "$@"; do
    case "$arg" in
        --windows) windows=1 ;;
        --android) android=1 ;;
        --no-linux) linux=0 ;;
        *) sed -n '2,10p' "$0"; exit 1 ;;
    esac
done

if [ $linux = 1 ]; then
    cargo build --release -p cpz-app
    publish linux-x86_64 target/release/cpz-music cpz-music
fi

if [ $windows = 1 ]; then
    cargo build --release -p cpz-app --target x86_64-pc-windows-gnu
    publish windows-x86_64 target/x86_64-pc-windows-gnu/release/cpz-music.exe cpz-music.exe
fi

if [ $android = 1 ]; then
    cargo android
    publish android-aarch64 "dist/cpz-music-$version.apk" cpz-music.apk
fi

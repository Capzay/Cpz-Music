#!/usr/bin/env bash
# Builds the desktop app and puts it on the server, so every running copy offers
# "Update and restart". Bump `version` in crates/app/Cargo.toml first: the app
# only updates to a version higher than its own.
#
#   scripts/publish-client.sh            # Linux x86_64
#   scripts/publish-client.sh --windows  # Windows x86_64 too (needs mingw-w64-gcc)
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
    echo "publishing $platform $version (was ${current:-none})"
    ssh "$HOST" "mkdir -p $DIR/$platform"
    scp -q "$binary" "$HOST:$DIR/$platform/$name"
    # The manifest goes last and is renamed into place, so the server never
    # points at a half-copied build.
    printf '{ "version": "%s", "file": "%s", "sha256": "%s" }\n' "$version" "$name" "$sha" |
        ssh "$HOST" "cat > $DIR/$platform/latest.json.new && mv $DIR/$platform/latest.json.new $DIR/$platform/latest.json && find $DIR/$platform -maxdepth 1 -name '${file%.exe}-*' ! -name '$name' -delete"
}

cargo build --release -p cpz-app
publish linux-x86_64 target/release/cpz-music cpz-music

if [ "${1:-}" = "--windows" ]; then
    cargo build --release -p cpz-app --target x86_64-pc-windows-gnu
    publish windows-x86_64 target/x86_64-pc-windows-gnu/release/cpz-music.exe cpz-music.exe
fi

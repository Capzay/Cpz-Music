#!/usr/bin/env bash
# Builds installers for the desktop app into dist/:
#
#   dist/cpz-music-<version>-linux-x86_64-setup.sh    run with `sh <file>`
#   dist/Cpz-Music-Setup-<version>.exe                double-click on Windows
#
#   scripts/package-client.sh             # both
#   scripts/package-client.sh --linux     # only Linux
#   scripts/package-client.sh --windows   # only Windows
#
# Both install per user (no sudo or admin), add the app to the menu, and the
# installed app keeps updating itself from the server as before. Uninstall:
# `sh <setup file> --uninstall` on Linux; Installed apps on Windows.
#
# Windows is cross-built with cargo-zigbuild (`cargo install cargo-zigbuild`,
# zig from `uv tool install ziglang`), so no mingw is needed.
set -euo pipefail
cd "$(dirname "$0")/.."

version=$(sed -n 's/^version = "\(.*\)"/\1/p' crates/app/Cargo.toml | head -1)
[ -n "$version" ] || { echo "no version in crates/app/Cargo.toml" >&2; exit 1; }

linux=1 windows=1
case "${1:-}" in
    "") ;;
    --linux) windows=0 ;;
    --windows) linux=0 ;;
    *) sed -n '2,16p' "$0"; exit 1 ;;
esac
mkdir -p dist

if [ $linux = 1 ]; then
    cargo build --release -p cpz-app
    out="dist/cpz-music-$version-linux-x86_64-setup.sh"
    # A short script with the binary appended after the marker line. It
    # unpacks the binary to a temporary folder and lets it install itself.
    cat > "$out" <<EOF
#!/bin/sh
# Cpz Music $version for Linux x86_64.
#   sh $(basename "$out")              install (to ~/.local, no sudo)
#   sh $(basename "$out") --uninstall  remove (downloads and settings are kept)
set -e
tmp=\$(mktemp -d)
trap 'rm -rf "\$tmp"' EXIT
skip=\$(awk '/^__CPZ_BINARY__\$/ { print NR + 1; exit }' "\$0")
tail -n +"\$skip" "\$0" > "\$tmp/cpz-music"
chmod +x "\$tmp/cpz-music"
"\$tmp/cpz-music" "\${1:---install}"
exit \$?
__CPZ_BINARY__
EOF
    cat target/release/cpz-music >> "$out"
    chmod +x "$out"
    echo "built $out"
fi

if [ $windows = 1 ]; then
    cargo zigbuild --release -p cpz-app --target x86_64-pc-windows-gnu
    # The same program as the app; "Setup" in the name makes it install.
    out="dist/Cpz-Music-Setup-$version.exe"
    cp target/x86_64-pc-windows-gnu/release/cpz-music.exe "$out"
    echo "built $out"
fi

#!/bin/sh
# Extract the original MIDlet's resources into ./assets for OpenOBM.
#
#   sh tools/extract-assets.sh path/to/Oblivion.jar [dest]
#
# A .jar is a zip; the game's resources sit at its root. We take everything
# except the Java classes and the manifest.
set -eu

jar="${1:?usage: extract-assets.sh <Oblivion.jar> [dest]}"
dest="${2:-assets}"
[ -f "$jar" ] || { echo "No such file: $jar" >&2; exit 1; }

mkdir -p "$dest"
# -j junks paths (resources are at the root), -o overwrites, -x excludes.
unzip -joq "$jar" -d "$dest" -x '*.class' '*.jar' '*.jad' 'META-INF/*'

echo "Extracted $(find "$dest" -type f | wc -l | tr -d ' ') resource files to $dest/"
echo "Parser checks: cargo test -p eso-tools --features assets --locked"
echo "Playing also requires unpublished text masks; see the README setup limitation."

#!/usr/bin/env bash
#
# Build the macOS installer package for Sämpler.
#
#   packaging/macos/build-pkg.sh <version> <folder with the bundles>
#
# Produces dist/installer/Saempler-<version>.pkg.
#
# The package is not signed or notarised. Doing so needs an Apple Developer
# account and secrets this repository does not carry, so Gatekeeper will warn
# on first open; the release notes say how to get past it. Signing is a matter
# of adding `--sign` here and the certificate to the workflow.

set -euo pipefail

version="${1:?Version fehlt}"
source_dir="${2:?Quellordner fehlt}"

identifier="de.mcbernie.saempler"
out_dir="dist/installer"
staging="$(mktemp -d)"
trap 'rm -rf "$staging"' EXIT

# Each component installs into a different place, so each gets its own payload
# root and its own pkgbuild. A single root would need the three to share a
# prefix, which they do not.
build_component() {
    local name="$1" source="$2" install_to="$3" out="$4"

    if [ ! -e "$source" ]; then
        echo "übersprungen: $source fehlt" >&2
        return
    fi

    local root="$staging/$name"
    mkdir -p "$root"
    cp -R "$source" "$root/"

    pkgbuild \
        --identifier "$identifier.$name" \
        --version "$version" \
        --root "$root" \
        --install-location "$install_to" \
        "$out"
}

mkdir -p "$out_dir" "$staging/pkgs"

build_component vst3 "$source_dir/Sämpler.vst3" \
    "/Library/Audio/Plug-Ins/VST3" "$staging/pkgs/vst3.pkg"
build_component clap "$source_dir/Sämpler.clap" \
    "/Library/Audio/Plug-Ins/CLAP" "$staging/pkgs/clap.pkg"
build_component standalone "$source_dir/saempler" \
    "/Applications/Sämpler" "$staging/pkgs/standalone.pkg"

# The choices the installer offers, in the order they appear.
choices=""
outlines=""
for component in vst3 clap standalone; do
    [ -f "$staging/pkgs/$component.pkg" ] || continue
    outlines="$outlines    <line choice=\"$component\"/>
"
    choices="$choices  <choice id=\"$component\" title=\"$component\" visible=\"true\">
    <pkg-ref id=\"$identifier.$component\"/>
  </choice>
  <pkg-ref id=\"$identifier.$component\" version=\"$version\">$component.pkg</pkg-ref>
"
done

cat > "$staging/distribution.xml" <<XML
<?xml version="1.0" encoding="utf-8"?>
<installer-gui-script minSpecVersion="2">
  <title>Sämpler $version</title>
  <options customize="allow" require-scripts="false" hostArchitectures="x86_64,arm64"/>
  <license file="LICENSE"/>
  <choices-outline>
$outlines  </choices-outline>
$choices</installer-gui-script>
XML

cp LICENSE "$staging/LICENSE"

productbuild \
    --distribution "$staging/distribution.xml" \
    --package-path "$staging/pkgs" \
    --resources "$staging" \
    "$out_dir/Saempler-$version.pkg"

echo "geschrieben: $out_dir/Saempler-$version.pkg"

#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != Darwin ]]; then
  echo 'Run this script on macOS with the Xcode command line tools and Rust installed.' >&2
  exit 1
fi

cd "$(dirname "${BASH_SOURCE[0]}")/.."
export MACOSX_DEPLOYMENT_TARGET=13.0
cargo build --locked --release -p pocketcam --target aarch64-apple-darwin --target x86_64-apple-darwin

staging="$(mktemp -d)"
trap 'rm -rf -- "$staging"' EXIT
volume="$staging/WEBKAM"
app="$volume/WEBKAM.app"
iconset="$staging/WEBKAM.iconset"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" "$iconset"

lipo -create \
  "${CARGO_TARGET_DIR:-target}/aarch64-apple-darwin/release/pocketcam" \
  "${CARGO_TARGET_DIR:-target}/x86_64-apple-darwin/release/pocketcam" \
  -output "$app/Contents/MacOS/WEBKAM"
lipo -verify_arch arm64 x86_64 "$app/Contents/MacOS/WEBKAM"
cp macos/Info.plist "$app/Contents/"
cp THIRD-PARTY-NOTICES.txt "$app/Contents/Resources/"
cp macos/THIRD-PARTY-NOTICES.txt "$app/Contents/Resources/macOS-NOTICES.txt"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" assets/pocketcam-512.png --out "$iconset/icon_${size}x${size}.png" >/dev/null
  doubled=$((size * 2))
  sips -z "$doubled" "$doubled" assets/pocketcam-512.png --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil --convert icns --output "$app/Contents/Resources/WEBKAM.icns" "$iconset"
if [[ -n "${WEBKAM_SIGN_IDENTITY:-}" ]]; then
  codesign --force --options runtime --timestamp --sign "$WEBKAM_SIGN_IDENTITY" "$app"
else
  codesign --force --sign - "$app"
fi
codesign --verify --strict "$app"

helper="$volume/VirtualCamera4Mac-0.0.7.dmg"
curl --fail --silent --show-error --location --output "$helper" \
  https://github.com/m96-chan/VirtualCamera4Mac/releases/download/v0.0.7/VirtualCamera4Mac-0.0.7.dmg
echo "88846e42b6fdb96bd4b7d2a61a4b24d2600e413436d9c68c8ec8372d618126ee  $helper" | shasum -a 256 -c
cp "macos/Start Here.txt" macos/VirtualCamera4Mac-LICENSE.txt "$volume/"
ln -s /Applications "$volume/Applications"
mkdir -p dist
image="dist/WEBKAM-$(/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' macos/Info.plist)-macOS-universal.dmg"
hdiutil create -ov -fs HFS+ -format UDZO -volname WEBKAM -srcfolder "$volume" "$image"
hdiutil verify "$image"

if [[ -n "${WEBKAM_NOTARY_PROFILE:-}" ]]; then
  xcrun notarytool submit "$image" --keychain-profile "$WEBKAM_NOTARY_PROFILE" --wait
  xcrun stapler staple "$image"
fi
shasum -a 256 "$image" > "$image.sha256"

#!/bin/sh
# Run on a Mac with the iPhoneOS SDK to build an IPA for personal signing.
set -eu
root="$(cd "$(dirname "$0")/.." && pwd)"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
app="$stage/Payload/PocketCam.app"
sdk=$(xcrun -sdk iphoneos --show-sdk-version)
if [ "${sdk%%.*}" -lt 18 ]; then
  echo "WEBKAM requires the iPhoneOS 18 SDK or later." >&2
  exit 1
fi
mkdir -p "$app/CameraUI/icons" "$root/dist"
xcrun -sdk iphoneos clang -arch arm64 -miphoneos-version-min=18.0 -fobjc-arc -fblocks -Os \
  "$root/mobile/ios/main.m" "$root/mobile/ios/PhoneLink.m" "$root/mobile/ios/NativeCamera.m" \
  "$root/mobile/ios/CameraInterface.m" \
  -framework UIKit -framework WebKit -framework Foundation -framework CoreGraphics \
  -framework AVFoundation -framework AVKit -framework CoreMedia -framework CoreVideo \
  -framework CoreImage -framework ImageIO -framework QuartzCore \
  -Wl,-no_adhoc_codesign -o "$app/PocketCam"
cp "$root/mobile/ios/Info.plist" "$root/mobile/ios/"*.png "$app/"
cp "$root/mobile/home.html" "$app/CameraUI/"
cp "$root/mobile/ios/ui/camera.html" "$root/mobile/ios/ui/camera.js" "$root/public/style.css" "$app/CameraUI/"
cp "$root/assets/pocketcam-192.png" "$root/assets/studio-landscape.png" "$app/CameraUI/icons/"
(cd "$stage" && /usr/bin/ditto -c -k --keepParent Payload "$root/dist/WEBKAM-1.0.3-iPhone-unsigned.ipa")

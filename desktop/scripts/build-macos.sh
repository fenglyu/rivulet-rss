#!/bin/sh
set -eu

if [ "$(uname -s)" != "Darwin" ]; then
    echo "This packaging script requires macOS." >&2
    exit 1
fi

desktop_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
build_mode=${1:-release}
case "$build_mode" in
    debug) cargo build --manifest-path "$desktop_dir/Cargo.toml" --locked ;;
    release) cargo build --manifest-path "$desktop_dir/Cargo.toml" --locked --release ;;
    *) echo "Usage: $0 [debug|release]" >&2; exit 1 ;;
esac

bundle_dir="$desktop_dir/dist/Rivulet.app"
mkdir -p "$bundle_dir/Contents/MacOS"
cp "$desktop_dir/target/$build_mode/rivulet-desktop" "$bundle_dir/Contents/MacOS/rivulet-desktop.new"
# Replace the inode so macOS does not retain the previous executable's cached signature.
mv -f "$bundle_dir/Contents/MacOS/rivulet-desktop.new" "$bundle_dir/Contents/MacOS/rivulet-desktop"
cat > "$bundle_dir/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>Rivulet</string>
<key>CFBundleDisplayName</key><string>Rivulet</string>
<key>CFBundleIdentifier</key><string>com.rivulet.desktop</string>
<key>CFBundleExecutable</key><string>rivulet-desktop</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
<key>CFBundleVersion</key><string>1</string>
<key>LSMinimumSystemVersion</key><string>11.0</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
printf 'APPL????' > "$bundle_dir/Contents/PkgInfo"
codesign --force --sign - "$bundle_dir"
codesign --verify "$bundle_dir"
echo "Built $bundle_dir"

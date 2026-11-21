#!/usr/bin/env bash
# Build only the standalone Apple Silicon client, natively or with OSXCross.
set -euo pipefail

if [[ $# -ne 0 ]]; then
    echo 'Usage: tools/build_mac_client.sh (configuration is through environment variables)' >&2
    exit 1
fi
if [[ -n "${OSXCROSS_ROOT:-}" ]]; then
    OSXCROSS_ROOT="$(cd -- "$OSXCROSS_ROOT" && pwd)"
fi
if [[ -n "${SDKROOT:-}" ]]; then
    SDKROOT="$(cd -- "$SDKROOT" && pwd)"
fi
cd -- "$(dirname -- "$0")/.."

export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-11.0}"
if [[ ! "$MACOSX_DEPLOYMENT_TARGET" =~ ^[0-9]+\.[0-9]+(\.[0-9]+)?$ ]] \
    || (( 10#${MACOSX_DEPLOYMENT_TARGET%%.*} < 11 )); then
    echo 'Apple Silicon requires MACOSX_DEPLOYMENT_TARGET of 11.0 or newer.' >&2
    exit 1
fi
host="$(uname -s)"
case "$host" in
    Darwin)
        if ! command -v xcrun >/dev/null 2>&1 \
            || ! sdk="$(xcrun --sdk macosx --show-sdk-path 2>/dev/null)"; then
            echo 'Install Xcode Command Line Tools with: xcode-select --install' >&2
            exit 1
        fi
        export SDKROOT="${SDKROOT:-$sdk}"
        compiler="$(xcrun --sdk macosx --find clang)"
        archiver="$(xcrun --sdk macosx --find ar)"
        for tool in codesign ditto; do
            command -v "$tool" >/dev/null 2>&1 || { echo "Missing macOS tool: $tool" >&2; exit 1; }
        done
        ;;
    Linux)
        if [[ -z "${OSXCROSS_ROOT:-}" ]]; then
            echo 'Linux cross-build requires OSXCross with an arm64-capable macOS SDK.' >&2
            echo 'Set OSXCROSS_ROOT to its installation (the directory containing bin/ and SDK/).' >&2
            echo 'See https://github.com/tpoechtrager/osxcross for SDK and toolchain setup.' >&2
            exit 1
        fi
        # Accept either the installation directory or an OSXCross source checkout.
        if [[ ! -d "$OSXCROSS_ROOT/bin" && -d "$OSXCROSS_ROOT/target/bin" ]]; then
            OSXCROSS_ROOT="$OSXCROSS_ROOT/target"
        fi
        export PATH="$OSXCROSS_ROOT/bin:$PATH"
        shopt -s nullglob
        compilers=("$OSXCROSS_ROOT"/bin/arm64-apple-darwin*-clang "$OSXCROSS_ROOT"/bin/aarch64-apple-darwin*-clang)
        if [[ ${#compilers[@]} -eq 0 ]]; then
            echo 'No arm64 Apple Clang wrapper found in OSXCROSS_ROOT/bin.' >&2
            exit 1
        fi
        compiler="${compilers[0]}"
        archiver="${compiler%-clang}-ar"
        if [[ ! -x "$archiver" ]]; then
            archiver="$(command -v llvm-ar || true)"
        fi
        if [[ -z "${SDKROOT:-}" ]]; then
            sdks=("$OSXCROSS_ROOT"/SDK/MacOSX*.sdk)
            if [[ ${#sdks[@]} -eq 0 ]]; then
                echo 'No macOS SDK found under OSXCROSS_ROOT/SDK; set SDKROOT explicitly.' >&2
                exit 1
            fi
            SDKROOT="$(printf '%s\n' "${sdks[@]}" | sort -V | tail -n 1)"
        fi
        export SDKROOT
        for tool in rcodesign zip; do
            command -v "$tool" >/dev/null 2>&1 || {
                echo "Missing cross-build tool: $tool" >&2
                echo 'Install rcodesign with: cargo install --locked apple-codesign' >&2
                exit 1
            }
        done
        ;;
    *)
        echo 'Run this script on macOS, or on Linux with OSXCross.' >&2
        exit 1
        ;;
esac
for tool in "$compiler" "$archiver"; do
    if [[ ! -x "$tool" ]]; then
        echo "Missing Apple build tool: $tool" >&2
        exit 1
    fi
done
if [[ ! -d "$SDKROOT" ]]; then
    echo 'SDKROOT must point to an installed macOS SDK.' >&2
    exit 1
fi

export CC_aarch64_apple_darwin="$compiler"
export AR_aarch64_apple_darwin="$archiver"
# Force arm64 even on an Intel Mac; also keep SDK/deployment flags identical for
# Rust linking and any C compilation performed by dependencies.
export CC_SHELL_ESCAPED_FLAGS=1
export CFLAGS_aarch64_apple_darwin="${CFLAGS_aarch64_apple_darwin:-} -arch arm64 -isysroot \"$SDKROOT\" -mmacosx-version-min=$MACOSX_DEPLOYMENT_TARGET"
export CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER="$compiler"
export RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=-arch -C link-arg=arm64 -C strip=debuginfo"

target=aarch64-apple-darwin
if [[ ! -d "$(rustc --print target-libdir --target "$target")" ]]; then
    rustup target add "$target"
fi
build_root="${CARGO_TARGET_DIR:-$PWD/target}"
cargo build --locked --release --target-dir "$build_root" --target "$target" \
    --no-default-features --features client --bin mas-client

# Respect Cargo's custom target directory, while keeping distribution artifacts
# in the same predictable project-local location as the Windows build script.
build_dir="$build_root/$target/release"
package_id="$(cargo pkgid --locked --offline)"
version="${package_id##*@}"
version="${version%%-*}"
version="${version%%+*}"
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo 'Cannot determine the application version from Cargo.' >&2
    exit 1
fi
mkdir -p target/mac
staging="$(mktemp -d "$PWD/target/mac/.package.XXXXXX")"
trap 'rm -rf -- "$staging"' EXIT
app_name='Marsal a spion.app'
app="$staging/$app_name"
mkdir -p "$app/Contents/MacOS"
cp "$build_dir/mas-client" "$staging/mas-client"
chmod 755 "$staging/mas-client"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
    <key>CFBundleExecutable</key><string>mas-client</string>
    <key>CFBundleIdentifier</key><string>org.marsal-a-spion.client</string>
    <key>CFBundleName</key><string>Maršál a špión</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>CFBundleVersion</key><string>$version</string>
    <key>LSMinimumSystemVersion</key><string>$MACOSX_DEPLOYMENT_TARGET</string>
    <key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST

# ARM executables need signatures. Sign only after all executable modifications.
# Ad-hoc signing needs no Apple account or distribution certificate.
if [[ "$host" == Darwin ]]; then
    codesign --force --sign - "$staging/mas-client"
    cp "$staging/mas-client" "$app/Contents/MacOS/mas-client"
    codesign --force --sign - "$app"
    codesign --verify --strict "$app"
    ditto -c -k --sequesterRsrc --keepParent "$app" "$staging/mas-client-macos-arm64.zip"
else
    rcodesign sign "$staging/mas-client"
    cp "$staging/mas-client" "$app/Contents/MacOS/mas-client"
    rcodesign sign "$app"
    (cd -- "$staging" && zip -q -r mas-client-macos-arm64.zip "$app_name")
fi
# Replace only artifacts owned by this script, after building/packaging succeeds.
if [[ -e "target/mac/$app_name" ]]; then
    mv "target/mac/$app_name" "$staging/previous.app"
fi
mv "$app" "target/mac/$app_name"
mv "$staging/mas-client" target/mac/mas-client
mv "$staging/mas-client-macos-arm64.zip" target/mac/mas-client-macos-arm64.zip
printf '%s\n' 'Apple Silicon executable: target/mac/mas-client' \
    'Application bundle: target/mac/Marsal a spion.app' \
    'Shareable ZIP: target/mac/mas-client-macos-arm64.zip'

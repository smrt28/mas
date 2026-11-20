#!/usr/bin/env bash
# Cross-build only the standalone Windows client; cache build tools under target/.
set -euo pipefail
if [[ -n "${LLVM_MINGW_ROOT:-}" ]]; then
    LLVM_MINGW_ROOT="$(cd -- "$LLVM_MINGW_ROOT" && pwd)"
fi
cd -- "$(dirname -- "$0")/.."

if [[ -n "${LLVM_MINGW_ROOT:-}" ]]; then
    compiler="$LLVM_MINGW_ROOT/bin/x86_64-w64-mingw32-clang"
elif command -v x86_64-w64-mingw32-clang >/dev/null 2>&1; then
    compiler="$(command -v x86_64-w64-mingw32-clang)"
else
    if [[ "$(uname -sm)" != 'Linux x86_64' ]]; then
        echo 'Automatic LLVM-MinGW setup requires x86_64 Linux; otherwise set LLVM_MINGW_ROOT.' >&2
        exit 1
    fi
    version=20261117
    bundle="llvm-mingw-$version-ucrt-ubuntu-22.04-x86_64"
    checksum=e7a15a49a2ea95e9f3d8a9d0cfabe5052a4b10fb4777cc54de5a8c27b64bb701
    cache="$PWD/target/toolchains"
    root="$cache/$bundle"
    if [[ ! -x "$root/bin/x86_64-w64-mingw32-clang" ]]; then
        mkdir -p "$cache"
        archive="$cache/$bundle.tar.xz"
        if [[ ! -f "$archive" ]]; then
            echo "Downloading LLVM-MinGW $version (cached under target/toolchains)…"
            curl --fail --location --retry 3 \
                "https://github.com/mstorsjo/llvm-mingw/releases/download/$version/$bundle.tar.xz" \
                --output "$archive.part"
            printf '%s  %s\n' "$checksum" "$archive.part" | sha256sum --check --status
            mv -- "$archive.part" "$archive"
        fi
        printf '%s  %s\n' "$checksum" "$archive" | sha256sum --check --status
        staging="$root.extract-$$"
        mkdir -p "$staging"
        tar -xJf "$archive" -C "$staging" --strip-components=1
        mv -- "$staging" "$root"
    fi
    compiler="$root/bin/x86_64-w64-mingw32-clang"
fi

tool_bin="$(dirname -- "$compiler")"
for tool in "$compiler" "$tool_bin/llvm-ar" "$tool_bin/llvm-strip"; do
    if [[ ! -x "$tool" ]]; then
        echo "Missing LLVM-MinGW tool: $tool. Check LLVM_MINGW_ROOT." >&2
        exit 1
    fi
done
export PATH="$tool_bin:$PATH"
target=x86_64-pc-windows-gnullvm
if [[ ! -d "$(rustc --print target-libdir --target "$target")" ]]; then
    rustup target add "$target"
fi
export CC_x86_64_pc_windows_gnullvm="$compiler"
export AR_x86_64_pc_windows_gnullvm="$tool_bin/llvm-ar"
export CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_LINKER="$compiler"
export RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=+crt-static -C link-arg=-static -C strip=debuginfo"
cargo build --locked --release --target "$target" \
    --no-default-features --features client --bin mas-client
mkdir -p target/windows
cp "target/$target/release/mas-client.exe" target/windows/mas-client.exe
"$tool_bin/llvm-strip" --strip-all target/windows/mas-client.exe
printf '%s\n' 'Standalone Windows client: target/windows/mas-client.exe'

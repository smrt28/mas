#!/usr/bin/env bash
# Cross-build a standalone 64-bit Windows client with a relocatable LLVM-MinGW toolchain.
set -euo pipefail
cd -- "$(dirname -- "$0")/.."
if [[ -n "${LLVM_MINGW_ROOT:-}" ]]; then
    export PATH="$LLVM_MINGW_ROOT/bin:$PATH"
fi
compiler="$(command -v x86_64-w64-mingw32-clang || true)"
archiver="$(command -v llvm-ar || true)"
if [[ -z "$compiler" || -z "$archiver" ]]; then
    echo 'Install LLVM-MinGW and set LLVM_MINGW_ROOT to its extracted directory.' >&2
    exit 1
fi
export CC_x86_64_pc_windows_gnullvm="$compiler"
export AR_x86_64_pc_windows_gnullvm="$archiver"
export CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_LINKER="$compiler"
export RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=+crt-static -C link-arg=-static -C strip=debuginfo"
cargo build --locked --release --target x86_64-pc-windows-gnullvm \
    --no-default-features --features client --bin mas-client
mkdir -p target/windows
cp target/x86_64-pc-windows-gnullvm/release/mas-client.exe target/windows/mas-client.exe
llvm-strip --strip-all target/windows/mas-client.exe
printf '%s\n' 'Standalone Windows client: target/windows/mas-client.exe'

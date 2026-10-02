#!/bin/sh
# SPDX-License-Identifier: Apache-2.0
set -eu
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repository_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
cd "$repository_root"
if command -v x86_64-w64-mingw32-gcc >/dev/null 2>&1; then
    exec cargo clippy --all-targets --all-features --locked --target x86_64-pc-windows-gnu -- -D warnings
fi
if command -v zig >/dev/null 2>&1; then
    export CC_x86_64_pc_windows_gnu="$script_dir/zig-cc-windows.sh"
    export AR_x86_64_pc_windows_gnu="$script_dir/zig-ar-windows.sh"
    export ZIG_LOCAL_CACHE_DIR="$repository_root/target/zig-cache"
    export ZIG_GLOBAL_CACHE_DIR="$repository_root/target/zig-global-cache"
    exec cargo clippy --all-targets --all-features --locked --target x86_64-pc-windows-gnu -- -D warnings
fi
echo 'Windows GNU cross-check requires a MinGW C compiler or Zig.' >&2
exit 1

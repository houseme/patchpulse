#!/bin/sh
# SPDX-License-Identifier: Apache-2.0
set -eu
# cc-rs passes Rust's vendor triple, while Zig expects a target without `pc`.
for argument in "$@"; do
    shift
    case "$argument" in
        --target=x86_64-pc-windows-gnu) ;;
        *) set -- "$@" "$argument" ;;
    esac
done
exec zig cc -target x86_64-windows-gnu "$@"

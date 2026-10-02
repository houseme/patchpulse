#!/bin/sh
# SPDX-License-Identifier: Apache-2.0
# Copy the executable's actual dynamic runtime and its upstream notices.
set -eu
runtime_root=/runtime-root
mkdir -p "$runtime_root/usr/local/bin" "$runtime_root/etc/patchpulse" "$runtime_root/usr/share/doc/patchpulse/rust" "$runtime_root/usr/share/doc/patchpulse/system"
cp target/release/patchpulse "$runtime_root/usr/local/bin/patchpulse"
ldd target/release/patchpulse | awk '$2 == "=>" && $3 ~ /^\// { print $3 } $1 ~ /^\// { print $1 }' | sort -u > /tmp/patchpulse-runtime-libraries
while IFS= read -r library; do
    cp --parents --dereference "$library" "$runtime_root"
done < /tmp/patchpulse-runtime-libraries
cp -a "$(rustc --print sysroot)/share/doc/rust/." "$runtime_root/usr/share/doc/patchpulse/rust/"
cp -L /usr/share/doc/libc6/copyright "$runtime_root/usr/share/doc/patchpulse/system/libc6-copyright"
cp -L /usr/share/doc/libgcc-s1/copyright "$runtime_root/usr/share/doc/patchpulse/system/libgcc-s1-copyright"
mkdir -p "$runtime_root/etc/ssl/certs"
cp -L /etc/ssl/certs/ca-certificates.crt "$runtime_root/etc/ssl/certs/ca-certificates.crt"
cp -L /usr/share/doc/ca-certificates/copyright "$runtime_root/usr/share/doc/patchpulse/system/ca-certificates-copyright"

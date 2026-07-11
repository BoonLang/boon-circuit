#!/usr/bin/env bash
set -euo pipefail

export LC_ALL=C
umask 022

script_dir=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/../.." && pwd)
out_dir="$repo_root/target/tools/native-wayland-test"
generated_dir="$out_dir/generated"
cc_bin=${CC:-cc}

require_command() {
    if ! command -v "$1" >/dev/null 2>&1; then
        printf 'missing required command: %s\n' "$1" >&2
        exit 1
    fi
}

require_command "$cc_bin"
require_command pkg-config
require_command wayland-scanner

packages=(weston libweston-13 wayland-client wayland-server)
for package in "${packages[@]}"; do
    if ! pkg-config --exists "$package"; then
        printf 'missing required pkg-config package: %s\n' "$package" >&2
        exit 1
    fi
done

weston_version=$(pkg-config --modversion weston)
libweston_version=$(pkg-config --modversion libweston-13)
if [[ "$weston_version" != 13.0.0 || "$libweston_version" != 13.0.0 ]]; then
    printf 'Weston 13.0.0 required; found weston=%s libweston-13=%s\n' \
        "$weston_version" "$libweston_version" >&2
    exit 1
fi

rm -rf -- "$out_dir"
mkdir -p -- "$generated_dir"

protocol="$script_dir/weston-test-v1.xml"
wayland-scanner server-header "$protocol" \
    "$generated_dir/weston-test-server-protocol.h"
wayland-scanner client-header "$protocol" \
    "$generated_dir/weston-test-client-protocol.h"
wayland-scanner private-code "$protocol" \
    "$generated_dir/weston-test-protocol.c"

read -r -a module_cflags <<<"$(pkg-config --cflags weston libweston-13 wayland-server)"
read -r -a module_libs <<<"$(pkg-config --libs libweston-13 wayland-server)"
read -r -a driver_cflags <<<"$(pkg-config --cflags wayland-client)"
read -r -a driver_libs <<<"$(pkg-config --libs wayland-client)"

common_flags=(-std=c11 -O2 -g0 -Wall -Wextra -Werror -fno-common)
"$cc_bin" "${common_flags[@]}" -D_GNU_SOURCE -fPIC -shared \
    -Wl,-z,defs -I"$generated_dir" "${module_cflags[@]}" \
    "$script_dir/weston-test-module.c" \
    "$generated_dir/weston-test-protocol.c" "${module_libs[@]}" \
    -o "$out_dir/native-wayland-test-module.so"

"$cc_bin" "${common_flags[@]}" -D_POSIX_C_SOURCE=200809L \
    -I"$generated_dir" "${driver_cflags[@]}" \
    "$script_dir/native-wayland-test-driver.c" \
    "$generated_dir/weston-test-protocol.c" "${driver_libs[@]}" -lm \
    -o "$out_dir/native-wayland-test-driver"

printf 'built Weston %s tools in %s\n' "$weston_version" "$out_dir"

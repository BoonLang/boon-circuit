#!/usr/bin/env bash
set -euo pipefail

export LC_ALL=C
umask 077

script_dir=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/../.." && pwd)
out_dir="$repo_root/target/tools/native-wayland-test"
runtime_dir=
weston_pid=

cleanup() {
    status=$?
    trap - EXIT INT TERM
    if [[ -n "$weston_pid" ]] && kill -0 "$weston_pid" 2>/dev/null; then
        kill "$weston_pid" 2>/dev/null || true
        sleep 0.1
        if kill -0 "$weston_pid" 2>/dev/null; then
            kill -KILL "$weston_pid" 2>/dev/null || true
        fi
    fi
    if [[ -n "$weston_pid" ]]; then
        wait "$weston_pid" 2>/dev/null || true
    fi
    if [[ -n "$runtime_dir" ]]; then
        rm -rf -- "$runtime_dir"
    fi
    exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

if ! command -v weston >/dev/null 2>&1; then
    printf 'missing required command: weston\n' >&2
    exit 1
fi

"$script_dir/build.sh"

runtime_dir=$(mktemp -d "${TMPDIR:-/tmp}/native-wayland-test.XXXXXX")
socket="native-wayland-test-$$"
log="$runtime_dir/weston.log"
query="$runtime_dir/query.txt"

XDG_RUNTIME_DIR="$runtime_dir" weston \
    --backend=headless-backend.so \
    --shell=fullscreen-shell.so \
    --socket="$socket" \
    --idle-time=0 \
    --no-config \
    --log="$log" \
    --modules="$out_dir/native-wayland-test-module.so" &
weston_pid=$!

ready=false
for _ in {1..50}; do
    if XDG_RUNTIME_DIR="$runtime_dir" WAYLAND_DISPLAY="$socket" \
        "$out_dir/native-wayland-test-driver" query >"$query" 2>/dev/null; then
        ready=true
        break
    fi
    if ! kill -0 "$weston_pid" 2>/dev/null; then
        break
    fi
    sleep 0.1
done

if [[ "$ready" != true ]]; then
    printf 'headless Weston did not expose weston_test v1\n' >&2
    if [[ -f "$log" ]]; then
        sed -n '1,160p' "$log" >&2
    fi
    exit 1
fi

printf 'smoke pass: %s\n' "$(<"$query")"

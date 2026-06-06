#!/usr/bin/env bash
set -euo pipefail

if command -v cargo >/dev/null 2>&1; then
  exec cargo "$@"
fi

to_windows_path() {
  if command -v cygpath >/dev/null 2>&1; then
    cygpath -w "$1"
  elif command -v wslpath >/dev/null 2>&1; then
    wslpath -w "$1"
  else
    printf '%s\n' "$1"
  fi
}

rewrite_args_for_windows_cargo() {
  local rewritten=()
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --manifest-path)
        rewritten+=("$1")
        shift
        rewritten+=("$(to_windows_path "$1")")
        ;;
      *)
        rewritten+=("$1")
        ;;
    esac
    shift
  done
  printf '%s\n' "${rewritten[@]}"
}

run_windows_cargo() {
  local cargo_bin="$1"
  shift
  local args=()
  while IFS= read -r arg; do
    args+=("$arg")
  done < <(rewrite_args_for_windows_cargo "$@")
  if "$cargo_bin" +stable-x86_64-pc-windows-gnu --version >/dev/null 2>&1; then
    args=("+stable-x86_64-pc-windows-gnu" "${args[@]}")
  fi
  exec "$cargo_bin" "${args[@]}"
}

if [ -n "${CARGO:-}" ] && [ -x "$CARGO" ]; then
  run_windows_cargo "$CARGO" "$@"
fi

if command -v cargo.exe >/dev/null 2>&1; then
  run_windows_cargo "$(command -v cargo.exe)" "$@"
fi

if [ -x /c/Users/BeiChen/.cargo/bin/cargo.exe ]; then
  run_windows_cargo /c/Users/BeiChen/.cargo/bin/cargo.exe "$@"
fi

if [ -x /mnt/c/Users/BeiChen/.cargo/bin/cargo.exe ]; then
  run_windows_cargo /mnt/c/Users/BeiChen/.cargo/bin/cargo.exe "$@"
fi

echo "cargo_not_found" >&2
exit 127

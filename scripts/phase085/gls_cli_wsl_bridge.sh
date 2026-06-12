#!/usr/bin/env bash
set -euo pipefail

win_exe='D:\Project\rust\GenoLigand\target\debug\gls-cli.exe'

to_windows_arg() {
  local arg="$1"
  if [[ "$arg" =~ ^/mnt/([a-zA-Z])/(.*)$ ]]; then
    local drive="${BASH_REMATCH[1]}"
    local rest="${BASH_REMATCH[2]}"
    drive="$(printf '%s' "$drive" | tr '[:lower:]' '[:upper:]')"
    rest="${rest//\//\\}"
    printf '%s:\\%s' "$drive" "$rest"
  else
    printf '%s' "$arg"
  fi
}

ps_command="& '$win_exe'"
for arg in "$@"; do
  converted="$(to_windows_arg "$arg")"
  converted="${converted//\'/\'\'}"
  ps_command+=" '$converted'"
done

/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe \
  -NoProfile \
  -ExecutionPolicy Bypass \
  -Command "$ps_command"

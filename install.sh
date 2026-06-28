#!/usr/bin/env sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
PYTHON=${GENOLIGAND_PYTHON:-python3}

install_python() {
  if command -v apt-get >/dev/null 2>&1; then
    SUDO=""
    if [ "$(id -u)" -ne 0 ]; then SUDO="sudo"; fi
    $SUDO apt-get update
    $SUDO apt-get install -y python3 python3-venv python3-pip ca-certificates curl
  elif command -v dnf >/dev/null 2>&1; then
    SUDO=""
    if [ "$(id -u)" -ne 0 ]; then SUDO="sudo"; fi
    $SUDO dnf install -y python3 python3-pip ca-certificates curl
  elif command -v yum >/dev/null 2>&1; then
    SUDO=""
    if [ "$(id -u)" -ne 0 ]; then SUDO="sudo"; fi
    $SUDO yum install -y python3 python3-pip ca-certificates curl
  elif command -v pacman >/dev/null 2>&1; then
    SUDO=""
    if [ "$(id -u)" -ne 0 ]; then SUDO="sudo"; fi
    $SUDO pacman -Sy --noconfirm python python-pip ca-certificates curl
  else
    echo "未找到 Python，也未识别可用的系统包管理器。" >&2
    exit 127
  fi
}

if ! command -v "$PYTHON" >/dev/null 2>&1; then
  install_python
fi

exec "$PYTHON" "$ROOT/scripts/setup/install_complete_scientific.py" --workspace-root "$ROOT" --start-webui "$@"

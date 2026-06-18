#!/usr/bin/env sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
PYTHON=${GENOLIGAND_PYTHON:-python3}

exec "$PYTHON" "$ROOT/scripts/setup/install_complete_scientific.py" --workspace-root "$ROOT" --start-webui "$@"

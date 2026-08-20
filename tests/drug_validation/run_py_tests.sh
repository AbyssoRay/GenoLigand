#!/bin/bash
cd /mnt/d/Project/rust/GenoLigand/python
export LD_LIBRARY_PATH=/usr/lib/wsl/lib:/usr/local/cuda/lib64
export PYTHONPATH=/mnt/d/Project/rust/GenoLigand/python
/root/.local/state/genoligand/python-worker-env/bin/python -m unittest discover -s tests > /tmp/pytest_out.txt 2>&1
echo "=== RESULT ==="
grep -E "FAIL|ERROR|Ran " /tmp/pytest_out.txt | head -40

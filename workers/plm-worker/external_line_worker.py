#!/usr/bin/env python3
"""Persistent pLM line-worker adapter.

Set GENOLIGAND_PLM_EMBED_COMMAND to a long-lived command that reads EMBED
requests from stdin and writes one OK/ERR/raw response line per request.
"""

import os
import subprocess
import sys


def respond_ok(payload: str = "") -> None:
    sys.stdout.write(f"OK\t{payload}\n" if payload else "OK\n")
    sys.stdout.flush()


def respond_err(reason: str) -> None:
    sys.stdout.write(f"ERR\t{reason}\n")
    sys.stdout.flush()


class ExternalLineProcess:
    def __init__(self, command: str) -> None:
        self.process = subprocess.Popen(
            command,
            text=True,
            shell=True,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )

    def request(self, request: str) -> str:
        if self.process.stdin is None or self.process.stdout is None:
            raise RuntimeError("plm_external_pipe_unavailable")
        if self.process.poll() is not None:
            stderr = self.process.stderr.read().strip() if self.process.stderr else ""
            raise RuntimeError(f"plm_external_command_exited:{self.process.returncode}:{stderr}")
        self.process.stdin.write(request + "\n")
        self.process.stdin.flush()
        output = self.process.stdout.readline()
        if output == "":
            stderr = self.process.stderr.read().strip() if self.process.stderr else ""
            raise RuntimeError(f"plm_external_command_closed_stdout:{stderr}")
        output = output.strip()
        if output.startswith("OK\t"):
            return output[3:]
        if output == "OK":
            return ""
        if output.startswith("ERR\t"):
            raise RuntimeError(output[4:])
        return output

    def close(self) -> None:
        if self.process.poll() is None:
            self.process.terminate()


def main() -> int:
    command = os.environ.get("GENOLIGAND_PLM_EMBED_COMMAND")
    if not command:
        for _ in sys.stdin:
            respond_err("plm_embed_command_not_configured")
        return 0
    external = ExternalLineProcess(command)
    for line in sys.stdin:
        request = line.rstrip("\r\n")
        if not request.startswith("EMBED\t"):
            respond_err("plm_worker_bad_request")
            continue
        try:
            respond_ok(external.request(request))
        except Exception as exc:  # noqa: BLE001 - worker must convert all failures.
            respond_err(str(exc))
    external.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

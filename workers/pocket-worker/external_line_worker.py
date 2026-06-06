#!/usr/bin/env python3
"""Persistent pocket-worker adapter.

Set GENOLIGAND_POCKET_COMMAND to a long-lived command that reads POCKET
requests from stdin and writes one OK/ERR/raw node-payload line per request.
"""

import os
import subprocess
import sys

if hasattr(sys.stdin, "reconfigure"):
    sys.stdin.reconfigure(encoding="utf-8")
    sys.stdout.reconfigure(encoding="utf-8")


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
            raise RuntimeError("pocket_external_pipe_unavailable")
        if self.process.poll() is not None:
            stderr = self.process.stderr.read().strip() if self.process.stderr else ""
            raise RuntimeError(f"pocket_external_command_exited:{self.process.returncode}:{stderr}")
        self.process.stdin.write(request + "\n")
        self.process.stdin.flush()
        output = self.process.stdout.readline()
        if output == "":
            stderr = self.process.stderr.read().strip() if self.process.stderr else ""
            raise RuntimeError(f"pocket_external_command_closed_stdout:{stderr}")
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
    command = os.environ.get("GENOLIGAND_POCKET_COMMAND")
    if not command:
        for _ in sys.stdin:
            respond_err("pocket_command_not_configured")
        return 0
    external = ExternalLineProcess(command)
    for line in sys.stdin:
        request = line.lstrip("\ufeff").rstrip("\r\n")
        if not request.startswith("POCKET\t"):
            respond_err("pocket_worker_bad_request")
            continue
        try:
            respond_ok(external.request(request))
        except Exception as exc:  # noqa: BLE001
            respond_err(str(exc))
    external.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

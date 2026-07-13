from __future__ import annotations

from dataclasses import dataclass
from typing import Any


EXECUTION_RESULT_STATUS = {
    "completed": "PASS",
    "not_applicable": "NOT_APPLICABLE",
    "not_ready": "NOT_READY",
    "failed": "FAIL",
}


@dataclass(frozen=True)
class WorkerOutcome:
    status: str
    reason: str
    required: bool
    database_id: str
    row_count: int

    def __post_init__(self) -> None:
        if self.status not in EXECUTION_RESULT_STATUS:
            raise ValueError(f"unsupported worker outcome status: {self.status}")
        if not self.reason or not self.database_id or self.row_count < 0:
            raise ValueError("worker outcome fields must be complete")

    def as_dict(self) -> dict[str, Any]:
        return {
            "status": self.status,
            "reason": self.reason,
            "required": self.required,
            "database_id": self.database_id,
            "row_count": self.row_count,
        }


@dataclass(frozen=True)
class WorkerExecution:
    status: str
    reason: str
    engine_version: str
    outcomes: tuple[WorkerOutcome, ...]

    def __post_init__(self) -> None:
        if self.status not in EXECUTION_RESULT_STATUS:
            raise ValueError(f"unsupported worker execution status: {self.status}")
        if not self.reason or not self.engine_version or not self.outcomes:
            raise ValueError("worker execution fields must be complete")

    @property
    def result_status(self) -> str:
        return EXECUTION_RESULT_STATUS[self.status]

    def outcome_dicts(self) -> list[dict[str, Any]]:
        return [outcome.as_dict() for outcome in self.outcomes]

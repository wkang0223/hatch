"""Job and JobResult data classes."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import TYPE_CHECKING, Iterator

if TYPE_CHECKING:
    from neuralmesh.client import NeuralMeshClient


@dataclass
class Job:
    """Represents a NeuralMesh compute job."""

    id: str
    state: str
    runtime: str
    min_ram_gb: int
    max_price_per_hour: float
    created_at: str | None = None
    started_at: str | None = None
    completed_at: str | None = None
    provider_id: str | None = None
    exit_code: int | None = None
    actual_cost_nmc: float | None = None
    _client: "NeuralMeshClient | None" = field(default=None, repr=False)

    def refresh(self) -> "Job":
        """Fetch the latest state from the coordinator."""
        if self._client is None:
            raise RuntimeError("Job has no client — use the client directly.")
        updated = self._client.get_job(self.id)
        self.state = updated.state
        self.started_at = updated.started_at
        self.completed_at = updated.completed_at
        self.exit_code = updated.exit_code
        self.actual_cost_nmc = updated.actual_cost_nmc
        self.provider_id = updated.provider_id
        return self

    def stream_logs(self) -> Iterator[str]:
        """Stream stdout/stderr from the running job."""
        if self._client is None:
            raise RuntimeError("Job has no client — use the client directly.")
        yield from self._client.stream_logs(self.id)

    def wait(self, poll_interval: float = 5.0) -> "JobResult":
        """Block until this job reaches a terminal state and return the result."""
        if self._client is None:
            raise RuntimeError("Job has no client — use the client directly.")
        return self._client.wait_for_job(self.id, poll_interval=poll_interval)

    def cancel(self) -> None:
        """Cancel this job."""
        if self._client is None:
            raise RuntimeError("Job has no client — use the client directly.")
        self._client.cancel_job(self.id)
        self.state = "cancelled"

    @property
    def is_running(self) -> bool:
        return self.state == "running"

    @property
    def is_complete(self) -> bool:
        return self.state == "complete"

    @property
    def succeeded(self) -> bool:
        return self.state == "complete" and self.exit_code == 0

    def __repr__(self) -> str:
        return f"Job(id={self.id!r}, state={self.state!r}, runtime={self.runtime!r})"


@dataclass
class JobResult:
    """Terminal result of a job."""

    job_id: str
    state: str
    exit_code: int | None
    actual_cost_nmc: float | None

    @property
    def succeeded(self) -> bool:
        return self.state == "complete" and self.exit_code == 0

    def __repr__(self) -> str:
        return (
            f"JobResult(id={self.job_id!r}, state={self.state!r}, "
            f"exit_code={self.exit_code!r}, cost={self.actual_cost_nmc})"
        )

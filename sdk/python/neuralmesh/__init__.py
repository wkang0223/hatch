"""
NeuralMesh Python SDK
~~~~~~~~~~~~~~~~~~~~~

Decentralized Apple Silicon GPU marketplace.

Quick start::

    import neuralmesh as nm

    nm.configure(account_id="your-account-id")

    # Browse available providers
    providers = nm.list_providers(min_ram_gb=48, runtime="mlx")
    for p in providers:
        print(p.chip, p.memory_gb, "GB @", p.price_per_hour, "NMC/hr")

    # Submit a job
    job = nm.submit(
        script="./inference.py",
        runtime="mlx",
        ram_gb=48,
        hours=2,
    )
    print("Job ID:", job.id)

    # Stream logs
    for line in job.stream_logs():
        print(line, end="")

    # Wait for completion
    result = job.wait()
    print("Exit code:", result.exit_code)
"""

from neuralmesh.client import NeuralMeshClient, configure, get_client
from neuralmesh.job import Job, JobResult
from neuralmesh.provider import Provider

# Module-level convenience functions that delegate to the global client

def configure(
    account_id: str | None = None,
    coordinator_url: str = "https://coord1.neuralmesh.io:8080",
    ledger_url: str = "https://ledger.neuralmesh.io:8082",
    api_key: str | None = None,
) -> None:
    """Configure the global NeuralMesh client."""
    from neuralmesh.client import _configure
    _configure(
        account_id=account_id,
        coordinator_url=coordinator_url,
        ledger_url=ledger_url,
        api_key=api_key,
    )


def list_providers(
    min_ram_gb: int = 0,
    runtime: str | None = None,
    max_price: float | None = None,
    sort: str = "price",
    limit: int = 20,
) -> list[Provider]:
    """Return available providers matching the given filters."""
    return get_client().list_providers(
        min_ram_gb=min_ram_gb,
        runtime=runtime,
        max_price=max_price,
        sort=sort,
        limit=limit,
    )


def submit(
    script: str,
    runtime: str = "mlx",
    ram_gb: int = 16,
    hours: float = 1.0,
    max_price: float = 0.5,
    include: list[str] | None = None,
) -> Job:
    """Submit a compute job to the NeuralMesh network.

    Args:
        script:    Path to the Python script to run.
        runtime:   ML runtime (mlx, torch-mps, onnx-coreml, llama-cpp, shell).
        ram_gb:    Minimum unified memory required (GB).
        hours:     Maximum job duration (hours).
        max_price: Maximum price willing to pay (NMC/hr).
        include:   Additional files to bundle with the script.

    Returns:
        A :class:`Job` instance.
    """
    return get_client().submit(
        script=script,
        runtime=runtime,
        ram_gb=ram_gb,
        hours=hours,
        max_price=max_price,
        include=include or [],
    )


def list_jobs(limit: int = 20, state: str | None = None) -> list[Job]:
    """List jobs for the configured account."""
    return get_client().list_jobs(limit=limit, state=state)


def get_job(job_id: str) -> Job:
    """Get a job by ID."""
    return get_client().get_job(job_id)


def cancel_job(job_id: str) -> None:
    """Cancel a running or queued job."""
    return get_client().cancel_job(job_id)


def balance() -> float:
    """Return available NMC credit balance."""
    return get_client().get_balance()


__version__ = "0.1.0"
__all__ = [
    "configure",
    "list_providers",
    "submit",
    "list_jobs",
    "get_job",
    "cancel_job",
    "balance",
    "NeuralMeshClient",
    "Job",
    "JobResult",
    "Provider",
]

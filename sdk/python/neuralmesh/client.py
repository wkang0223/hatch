"""NeuralMesh HTTP client — wraps coordinator + ledger REST APIs."""

from __future__ import annotations

import hashlib
import os
import time
from pathlib import Path
from typing import Iterator

import httpx

from neuralmesh.job import Job, JobResult
from neuralmesh.provider import Provider

_GLOBAL_CLIENT: "NeuralMeshClient | None" = None


def _configure(
    account_id: str | None,
    coordinator_url: str,
    ledger_url: str,
    api_key: str | None,
) -> None:
    global _GLOBAL_CLIENT
    _GLOBAL_CLIENT = NeuralMeshClient(
        account_id=account_id,
        coordinator_url=coordinator_url,
        ledger_url=ledger_url,
        api_key=api_key,
    )


def get_client() -> "NeuralMeshClient":
    global _GLOBAL_CLIENT
    if _GLOBAL_CLIENT is None:
        # Auto-configure from environment / config file
        account_id = os.environ.get("NM_ACCOUNT_ID") or _load_config_account_id()
        _GLOBAL_CLIENT = NeuralMeshClient(account_id=account_id)
    return _GLOBAL_CLIENT


def configure(
    account_id: str | None = None,
    coordinator_url: str = "https://coord1.neuralmesh.io:8080",
    ledger_url: str = "https://ledger.neuralmesh.io:8082",
    api_key: str | None = None,
) -> None:
    _configure(account_id, coordinator_url, ledger_url, api_key)


def _load_config_account_id() -> str | None:
    """Read account_id from ~/.config/neuralmesh/cli.toml if present."""
    try:
        import tomllib  # Python 3.11+
    except ImportError:
        try:
            import tomli as tomllib  # type: ignore[no-redef]
        except ImportError:
            return None

    cfg_path = Path.home() / ".config" / "neuralmesh" / "cli.toml"
    if cfg_path.exists():
        with open(cfg_path, "rb") as f:
            data = tomllib.load(f)
        return data.get("account_id")
    return None


class NeuralMeshClient:
    """Synchronous REST client for the NeuralMesh coordinator and ledger."""

    def __init__(
        self,
        account_id: str | None = None,
        coordinator_url: str = "https://coord1.neuralmesh.io:8080",
        ledger_url: str = "https://ledger.neuralmesh.io:8082",
        api_key: str | None = None,
        timeout: float = 30.0,
    ) -> None:
        self.account_id = account_id or os.environ.get("NM_ACCOUNT_ID", "")
        self.coordinator_url = coordinator_url.rstrip("/")
        self.ledger_url = ledger_url.rstrip("/")

        headers = {}
        if api_key:
            headers["Authorization"] = f"Bearer {api_key}"

        self._http = httpx.Client(
            timeout=timeout,
            headers=headers,
            follow_redirects=True,
        )

    def _require_account(self) -> str:
        if not self.account_id:
            raise RuntimeError(
                "No account_id configured. Call neuralmesh.configure(account_id='...') "
                "or set the NM_ACCOUNT_ID environment variable."
            )
        return self.account_id

    # ── Providers ─────────────────────────────────────────────────────────────

    def list_providers(
        self,
        min_ram_gb: int = 0,
        runtime: str | None = None,
        max_price: float | None = None,
        sort: str = "price",
        limit: int = 20,
    ) -> list[Provider]:
        params: dict[str, str] = {"sort": sort, "limit": str(limit)}
        if min_ram_gb: params["min_ram_gb"] = str(min_ram_gb)
        if runtime:    params["runtime"] = runtime
        if max_price:  params["max_price"] = str(max_price)

        resp = self._http.get(f"{self.coordinator_url}/api/v1/providers", params=params)
        resp.raise_for_status()
        data = resp.json()
        return [Provider(**p) for p in data.get("providers", [])]

    # ── Jobs ─────────────────────────────────────────────────────────────────

    def submit(
        self,
        script: str,
        runtime: str = "mlx",
        ram_gb: int = 16,
        hours: float = 1.0,
        max_price: float = 0.5,
        include: list[str] | None = None,
    ) -> Job:
        account_id = self._require_account()
        script_path = Path(script)
        if not script_path.exists():
            raise FileNotFoundError(f"Script not found: {script}")

        # Collect files
        files: list[Path] = [script_path]
        for extra in (include or []):
            p = Path(extra)
            if p.exists():
                files.append(p)

        # Compute bundle hash
        h = hashlib.sha256()
        for f in sorted(files, key=lambda p: p.name):
            h.update(f.name.encode())
            h.update(f.read_bytes())
        bundle_hash = h.hexdigest()

        # Upload bundle (multipart)
        multi: list[tuple] = [("bundle_hash", (None, bundle_hash))]
        opened = []
        try:
            for f in files:
                fh = open(f, "rb")
                opened.append(fh)
                multi.append(("files", (f.name, fh, "application/octet-stream")))

            resp = self._http.post(
                f"{self.coordinator_url}/api/v1/artifacts",
                files=multi,
            )
            resp.raise_for_status()
            bundle_url = resp.json()["url"]
        finally:
            for fh in opened:
                fh.close()

        # Submit job
        payload = {
            "account_id": account_id,
            "runtime": runtime,
            "min_ram_gb": ram_gb,
            "max_duration_secs": int(hours * 3600),
            "max_price_per_hour": max_price,
            "bundle_hash": bundle_hash,
            "bundle_url": bundle_url,
            "script_name": script_path.name,
        }
        resp = self._http.post(f"{self.coordinator_url}/api/v1/jobs", json=payload)
        resp.raise_for_status()
        result = resp.json()

        return Job(
            id=result["job_id"],
            state=result["state"],
            runtime=runtime,
            min_ram_gb=ram_gb,
            max_price_per_hour=max_price,
            created_at=None,
            _client=self,
        )

    def list_jobs(self, limit: int = 20, state: str | None = None) -> list[Job]:
        account_id = self._require_account()
        params: dict[str, str] = {"account_id": account_id, "limit": str(limit)}
        if state: params["state"] = state

        resp = self._http.get(f"{self.coordinator_url}/api/v1/jobs", params=params)
        resp.raise_for_status()
        return [
            Job(**{**j, "_client": self})
            for j in resp.json().get("jobs", [])
        ]

    def get_job(self, job_id: str) -> Job:
        resp = self._http.get(f"{self.coordinator_url}/api/v1/jobs/{job_id}")
        resp.raise_for_status()
        return Job(**{**resp.json(), "_client": self})

    def cancel_job(self, job_id: str) -> None:
        resp = self._http.delete(f"{self.coordinator_url}/api/v1/jobs/{job_id}")
        resp.raise_for_status()

    def get_job_logs(self, job_id: str, offset: int = 0) -> tuple[str, bool]:
        """Returns (output_chunk, is_complete)."""
        resp = self._http.get(
            f"{self.coordinator_url}/api/v1/jobs/{job_id}/logs",
            params={"offset": str(offset)},
        )
        resp.raise_for_status()
        data = resp.json()
        return data.get("output", ""), data.get("is_complete", False)

    def stream_logs(self, job_id: str) -> Iterator[str]:
        """Generator that yields log lines until the job completes."""
        offset = 0
        terminal = {"complete", "failed", "cancelled"}
        while True:
            chunk, done = self.get_job_logs(job_id, offset)
            if chunk:
                yield chunk
                offset += len(chunk)
            if done:
                break
            # Check if job has ended
            try:
                job = self.get_job(job_id)
                if job.state in terminal:
                    break
            except Exception:
                pass
            time.sleep(2)

    def wait_for_job(self, job_id: str, poll_interval: float = 5.0) -> JobResult:
        """Block until the job reaches a terminal state."""
        terminal = {"complete", "failed", "cancelled"}
        while True:
            job = self.get_job(job_id)
            if job.state in terminal:
                return JobResult(
                    job_id=job_id,
                    state=job.state,
                    exit_code=job.exit_code,
                    actual_cost_nmc=job.actual_cost_nmc,
                )
            time.sleep(poll_interval)

    # ── Ledger ────────────────────────────────────────────────────────────────

    def get_balance(self) -> float:
        account_id = self._require_account()
        resp = self._http.get(f"{self.ledger_url}/api/v1/balance/{account_id}")
        resp.raise_for_status()
        return resp.json().get("available_nmc", 0.0)

    def __repr__(self) -> str:
        return f"NeuralMeshClient(account={self.account_id!r}, coordinator={self.coordinator_url!r})"

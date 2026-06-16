"""Provider data class."""

from __future__ import annotations

from dataclasses import dataclass, field


@dataclass
class Provider:
    """An available Apple Silicon GPU provider on the network."""

    id: str
    chip_model: str
    unified_memory_gb: int
    gpu_cores: int
    installed_runtimes: list[str]
    floor_price_nmc_per_hour: float
    trust_score: float
    state: str = "available"
    region: str | None = None
    bandwidth_mbps: int | None = None
    max_job_ram_gb: int | None = None
    last_seen: str | None = None

    # Accept any extra fields from API without breaking
    _extra: dict = field(default_factory=dict, repr=False)

    def __init__(self, **kwargs: object) -> None:
        known = {
            "id", "chip_model", "unified_memory_gb", "gpu_cores",
            "installed_runtimes", "floor_price_nmc_per_hour", "trust_score",
            "state", "region", "bandwidth_mbps", "max_job_ram_gb", "last_seen",
        }
        for k in known:
            setattr(self, k, kwargs.get(k))  # type: ignore[arg-type]
        self._extra = {k: v for k, v in kwargs.items() if k not in known}

    @property
    def chip(self) -> str:
        """Short chip name."""
        return self.chip_model or "Unknown"

    @property
    def memory_gb(self) -> int:
        """Unified memory in GB."""
        return self.unified_memory_gb or 0

    @property
    def price_per_hour(self) -> float:
        """Floor price in NMC/hr."""
        return self.floor_price_nmc_per_hour or 0.0

    @property
    def available(self) -> bool:
        return self.state == "available"

    def can_run(self, ram_gb: int, runtime: str | None = None) -> bool:
        """True if this provider can handle a job with the given requirements."""
        ram_ok = (self.unified_memory_gb or 0) >= ram_gb + 4  # 4 GB OS reserve
        rt_ok  = runtime is None or runtime in (self.installed_runtimes or [])
        return self.available and ram_ok and rt_ok

    def __repr__(self) -> str:
        return (
            f"Provider(id={self.id!r}, chip={self.chip!r}, "
            f"memory_gb={self.memory_gb}, price={self.price_per_hour:.4f} NMC/hr)"
        )

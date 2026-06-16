"""neuralmesh CLI — thin wrapper around the nm Rust binary for Python users."""

import subprocess
import sys

import click


@click.group()
def main() -> None:
    """NeuralMesh — decentralized Apple Silicon GPU marketplace."""


@main.command()
@click.option("--min-ram", default=0, help="Minimum RAM (GB)")
@click.option("--runtime", default=None, help="Filter by runtime (mlx, torch-mps, ...)")
@click.option("--max-price", default=None, type=float, help="Max NMC/hr")
def list_gpus(min_ram: int, runtime: str | None, max_price: float | None) -> None:
    """Browse available Mac GPU providers."""
    import neuralmesh as nm
    providers = nm.list_providers(min_ram_gb=min_ram, runtime=runtime, max_price=max_price)
    if not providers:
        click.echo("No providers found. Try relaxing your filters.")
        return
    from rich.table import Table
    from rich.console import Console
    t = Table(title="Available Providers")
    t.add_column("ID", style="cyan", max_width=14)
    t.add_column("Chip", style="yellow")
    t.add_column("RAM GB", justify="right")
    t.add_column("GPU", justify="right")
    t.add_column("NMC/hr", justify="right", style="green")
    t.add_column("Trust")
    t.add_column("Runtimes")
    for p in providers:
        rts = "+".join(r.replace("torch-mps","mps").replace("onnx-coreml","onnx") for r in (p.installed_runtimes or []))
        t.add_row(
            p.id[:12] + "…",
            p.chip_model or "?",
            str(p.unified_memory_gb),
            str(p.gpu_cores),
            f"{p.floor_price_nmc_per_hour:.4f}",
            "★" * round(p.trust_score or 0),
            rts,
        )
    Console().print(t)


@main.command()
@click.argument("script")
@click.option("--runtime", default="mlx", show_default=True)
@click.option("--ram", default=16, show_default=True, help="Min RAM GB")
@click.option("--hours", default=1.0, show_default=True)
@click.option("--max-price", default=0.5, show_default=True, help="Max NMC/hr")
@click.option("--wait", is_flag=True, help="Block until job completes")
@click.option("--logs", is_flag=True, help="Stream logs after submission")
def submit(script: str, runtime: str, ram: int, hours: float, max_price: float, wait: bool, logs: bool) -> None:
    """Submit a job to the NeuralMesh network."""
    import neuralmesh as nm
    from rich.console import Console
    console = Console()
    with console.status(f"Submitting [cyan]{script}[/] ({runtime}, {ram} GB)…"):
        job = nm.submit(script=script, runtime=runtime, ram_gb=ram, hours=hours, max_price=max_price)
    console.print(f"[green]✓[/] Job submitted: [cyan]{job.id}[/]  state=[yellow]{job.state}[/]")

    if logs or wait:
        console.print("Streaming logs…\n" + "─" * 60)
        for chunk in job.stream_logs():
            sys.stdout.write(chunk)
            sys.stdout.flush()
        console.print("\n" + "─" * 60)

    if wait:
        result = job.wait()
        icon = "[green]✓[/]" if result.succeeded else "[red]✗[/]"
        console.print(f"{icon} Job {result.state} · exit={result.exit_code} · cost={result.actual_cost_nmc} NMC")


@main.command()
@click.option("--limit", default=20, show_default=True)
@click.option("--state", default=None)
def list_jobs(limit: int, state: str | None) -> None:
    """List your recent jobs."""
    import neuralmesh as nm
    from rich.table import Table
    from rich.console import Console
    jobs = nm.list_jobs(limit=limit, state=state)
    if not jobs:
        click.echo("No jobs found.")
        return
    t = Table(title=f"Your Jobs ({len(jobs)})")
    t.add_column("ID", style="cyan", max_width=14)
    t.add_column("State")
    t.add_column("Runtime")
    t.add_column("RAM GB", justify="right")
    t.add_column("Cost NMC", justify="right", style="green")
    for job in jobs:
        state_color = {"complete": "blue", "running": "green", "failed": "red"}.get(job.state, "yellow")
        t.add_row(
            job.id[:12] + "…",
            f"[{state_color}]{job.state}[/{state_color}]",
            job.runtime,
            str(job.min_ram_gb),
            f"{job.actual_cost_nmc:.4f}" if job.actual_cost_nmc is not None else "—",
        )
    Console().print(t)


@main.command()
@click.argument("job_id")
@click.option("--follow", "-f", is_flag=True)
def logs(job_id: str, follow: bool) -> None:
    """Stream logs for a job."""
    import neuralmesh as nm
    job = nm.get_job(job_id)
    if follow:
        for chunk in job.stream_logs():
            sys.stdout.write(chunk)
            sys.stdout.flush()
    else:
        client = nm.get_client()
        chunk, _ = client.get_job_logs(job_id)
        sys.stdout.write(chunk or "(no output yet)")


@main.command()
def balance() -> None:
    """Show NMC wallet balance."""
    import neuralmesh as nm
    from rich.console import Console
    bal = nm.balance()
    Console().print(f"Available balance: [green bold]{bal:.4f} NMC[/]")


if __name__ == "__main__":
    main()

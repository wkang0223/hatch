"use client";

import { useEffect, useState } from "react";
import { Cpu, Activity, Database, Zap } from "lucide-react";

interface Stats {
  available_providers: number;
  active_providers: number;
  total_available_ram_gb: number;
  running_jobs: number;
  completed_jobs: number;
}

// Mock fallback while API is unreachable
const MOCK: Stats = {
  available_providers: 0,
  active_providers: 0,
  total_available_ram_gb: 0,
  running_jobs: 0,
  completed_jobs: 0,
};

export default function NetworkStatsBar() {
  const [stats, setStats] = useState<Stats>(MOCK);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    fetch("/api/coordinator/v1/stats")
      .then((r) => (r.ok ? r.json() : MOCK))
      .then((data) => { setStats(data); setLoaded(true); })
      .catch(() => setLoaded(true));

    const id = setInterval(() => {
      fetch("/api/coordinator/v1/stats")
        .then((r) => (r.ok ? r.json() : null))
        .then((data) => { if (data) setStats(data); })
        .catch(() => {});
    }, 15_000);
    return () => clearInterval(id);
  }, []);

  const items = [
    { icon: Cpu,      label: "Available providers", value: loaded ? stats.available_providers : "—", color: "text-brand-400" },
    { icon: Database, label: "Total GPU memory",     value: loaded ? `${stats.total_available_ram_gb} GB` : "—", color: "text-purple-400" },
    { icon: Activity, label: "Running jobs",         value: loaded ? stats.running_jobs : "—", color: "text-green-400" },
    { icon: Zap,      label: "Completed jobs",       value: loaded ? stats.completed_jobs.toLocaleString() : "—", color: "text-yellow-400" },
  ];

  return (
    <div className="grid grid-cols-2 md:grid-cols-4 gap-3 max-w-3xl mx-auto">
      {items.map((item) => (
        <div
          key={item.label}
          className="glass rounded-xl px-4 py-3 flex flex-col items-center gap-1 text-center"
        >
          <item.icon className={`h-4 w-4 ${item.color} mb-0.5`} />
          <div className={`text-2xl font-bold font-mono ${item.color}`}>
            {item.value}
          </div>
          <div className="text-xs text-slate-500">{item.label}</div>
        </div>
      ))}
    </div>
  );
}

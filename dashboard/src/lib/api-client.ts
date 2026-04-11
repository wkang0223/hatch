// NeuralMesh REST API client for the dashboard.
// Coordinator = provider registry + job matching.
// Ledger = NMC credit accounting.

const COORDINATOR =
  process.env.NEXT_PUBLIC_COORDINATOR_URL ?? "http://localhost:8080";
const LEDGER =
  process.env.NEXT_PUBLIC_LEDGER_URL ?? "http://localhost:8082";

// ── Types ─────────────────────────────────────────────────────────────────────

export interface Provider {
  id: string;
  chip_model: string;
  unified_memory_gb: number;
  gpu_cores: number;
  installed_runtimes: string[];
  floor_price_nmc_per_hour: number;
  trust_score: number;
  region?: string;
  bandwidth_mbps?: number;
  max_job_ram_gb?: number;
  state: "available" | "leased" | "offline";
  last_seen: string;
}

export interface Job {
  id: string;
  account_id: string;
  state: "queued" | "matching" | "assigned" | "running" | "complete" | "failed" | "cancelled";
  runtime: string;
  min_ram_gb: number;
  max_price_per_hour: number;
  bundle_hash?: string;
  provider_id?: string;
  created_at: string;
  started_at?: string;
  completed_at?: string;
  exit_code?: number;
  actual_cost_nmc?: number;
}

export interface NetworkStats {
  available_providers: number;
  active_providers: number;
  total_available_ram_gb: number;
  running_jobs: number;
  completed_jobs: number;
}

export interface Balance {
  account_id: string;
  available_nmc: number;
  escrowed_nmc: number;
  total_earned_nmc: number;
  total_spent_nmc: number;
}

export interface Transaction {
  id: string;
  kind: string;
  amount_nmc: number;
  balance_after: number;
  description: string;
  created_at: string;
}

export interface JobSubmitRequest {
  account_id: string;
  runtime: string;
  min_ram_gb: number;
  max_duration_secs: number;
  max_price_per_hour: number;
  bundle_hash?: string;
  bundle_url?: string;
  script_name: string;
}

export interface JobLogs {
  job_id: string;
  output: string;
  is_complete: boolean;
}

// ── Fetch helpers ─────────────────────────────────────────────────────────────

async function get<T>(url: string): Promise<T> {
  const res = await fetch(url, { next: { revalidate: 0 } });
  if (!res.ok) throw new Error(`GET ${url} → ${res.status}`);
  return res.json();
}

async function post<T>(url: string, body: unknown): Promise<T> {
  const res = await fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    const err = await res.text();
    throw new Error(err || `POST ${url} → ${res.status}`);
  }
  return res.json();
}

async function del(url: string): Promise<void> {
  const res = await fetch(url, { method: "DELETE" });
  if (!res.ok) throw new Error(`DELETE ${url} → ${res.status}`);
}

// ── Coordinator API ────────────────────────────────────────────────────────────

export interface ProviderListParams {
  min_ram_gb?: number;
  runtime?: string;
  max_price?: number;
  sort?: string;
  limit?: number;
}

export const api = {
  // Providers
  async listProviders(params: ProviderListParams = {}): Promise<{ providers: Provider[]; total: number }> {
    const q = new URLSearchParams();
    if (params.min_ram_gb) q.set("min_ram_gb", String(params.min_ram_gb));
    if (params.runtime)    q.set("runtime", params.runtime);
    if (params.max_price)  q.set("max_price", String(params.max_price));
    q.set("sort",  params.sort  ?? "price");
    q.set("limit", String(params.limit ?? 50));
    return get(`${COORDINATOR}/api/v1/providers?${q}`);
  },

  async getProvider(id: string): Promise<Provider> {
    return get(`${COORDINATOR}/api/v1/providers/${id}`);
  },

  // Network stats
  async getStats(): Promise<NetworkStats> {
    return get(`${COORDINATOR}/api/v1/stats`);
  },

  // Jobs
  async listJobs(accountId: string, limit = 20, state?: string): Promise<{ jobs: Job[]; total: number }> {
    const q = new URLSearchParams({ account_id: accountId, limit: String(limit) });
    if (state) q.set("state", state);
    return get(`${COORDINATOR}/api/v1/jobs?${q}`);
  },

  async getJob(id: string): Promise<Job> {
    return get(`${COORDINATOR}/api/v1/jobs/${id}`);
  },

  async submitJob(req: JobSubmitRequest): Promise<{ job_id: string; state: string; estimated_wait_secs: number }> {
    return post(`${COORDINATOR}/api/v1/jobs`, req);
  },

  async cancelJob(id: string): Promise<void> {
    return del(`${COORDINATOR}/api/v1/jobs/${id}`);
  },

  async getJobLogs(id: string, offset = 0): Promise<JobLogs> {
    return get(`${COORDINATOR}/api/v1/jobs/${id}/logs?offset=${offset}`);
  },

  // Ledger
  async getBalance(accountId: string): Promise<Balance> {
    return get(`${LEDGER}/api/v1/balance/${accountId}`);
  },

  async listTransactions(accountId: string, limit = 20): Promise<{ transactions: Transaction[]; total: number }> {
    return get(`${LEDGER}/api/v1/transactions?account_id=${accountId}&limit=${limit}`);
  },
};

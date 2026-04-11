-- NeuralMesh coordinator initial schema
-- Migration 001: providers, jobs, credits

CREATE TABLE IF NOT EXISTS providers (
    provider_id             TEXT PRIMARY KEY,
    chip_model              TEXT,
    unified_memory_gb       INTEGER,
    gpu_cores               INTEGER,
    cpu_cores               INTEGER,
    metal_version           TEXT,
    serial_number           TEXT,
    installed_runtimes      TEXT[],
    max_job_ram_gb          INTEGER,
    bandwidth_mbps          INTEGER,
    region                  TEXT,
    floor_price_nmc_per_hour DOUBLE PRECISION DEFAULT 0.05,
    wireguard_public_key    TEXT,
    state                   TEXT DEFAULT 'offline',  -- offline|available|leased|paused
    gpu_util_pct            DOUBLE PRECISION DEFAULT 0,
    ram_used_gb             INTEGER DEFAULT 0,
    active_job_id           TEXT,
    trust_score             DOUBLE PRECISION DEFAULT 3.0,
    jobs_completed          INTEGER DEFAULT 0,
    success_rate            DOUBLE PRECISION DEFAULT 1.0,
    nm_version              TEXT,
    last_seen               TIMESTAMPTZ,
    created_at              TIMESTAMPTZ DEFAULT now(),
    updated_at              TIMESTAMPTZ DEFAULT now()
);

CREATE TABLE IF NOT EXISTS jobs (
    job_id                  TEXT PRIMARY KEY,
    consumer_id             TEXT NOT NULL,
    provider_id             TEXT REFERENCES providers(provider_id),
    runtime                 INTEGER,        -- maps to Runtime enum
    min_ram_gb              INTEGER,
    max_duration_s          INTEGER,
    max_price_per_hour      DOUBLE PRECISION,
    price_per_hour          DOUBLE PRECISION,
    bundle_hash             TEXT,
    bundle_url              TEXT,
    consumer_ssh_pubkey     TEXT,
    consumer_wg_pubkey      TEXT,
    preferred_region        TEXT,
    state                   TEXT DEFAULT 'queued',  -- queued|matching|assigned|running|migrating|complete|failed|cancelled
    output_hash             TEXT,
    actual_runtime_s        BIGINT,
    wireguard_endpoint      TEXT,
    ssh_port                INTEGER DEFAULT 2222,
    started_at              TIMESTAMPTZ,
    completed_at            TIMESTAMPTZ,
    created_at              TIMESTAMPTZ DEFAULT now()
);

CREATE TABLE IF NOT EXISTS credit_accounts (
    account_id              TEXT PRIMARY KEY,  -- provider or consumer libp2p PeerId
    available_nmc           DOUBLE PRECISION DEFAULT 0.0,
    escrowed_nmc            DOUBLE PRECISION DEFAULT 0.0,
    total_earned_nmc        DOUBLE PRECISION DEFAULT 0.0,
    total_spent_nmc         DOUBLE PRECISION DEFAULT 0.0,
    created_at              TIMESTAMPTZ DEFAULT now(),
    updated_at              TIMESTAMPTZ DEFAULT now()
);

CREATE TABLE IF NOT EXISTS escrows (
    escrow_id               TEXT PRIMARY KEY,
    job_id                  TEXT REFERENCES jobs(job_id),
    consumer_id             TEXT NOT NULL,
    provider_id             TEXT,
    locked_nmc              DOUBLE PRECISION NOT NULL,
    state                   TEXT DEFAULT 'locked',  -- locked|released|slashed
    created_at              TIMESTAMPTZ DEFAULT now(),
    settled_at              TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS transactions (
    tx_id                   TEXT PRIMARY KEY,
    account_id              TEXT NOT NULL,
    tx_type                 TEXT NOT NULL,  -- deposit|withdraw|escrow_lock|escrow_release|earn|fee
    amount_nmc              DOUBLE PRECISION NOT NULL,
    reference               TEXT,
    description             TEXT,
    created_at              TIMESTAMPTZ DEFAULT now()
);

-- Indexes for common queries
CREATE INDEX IF NOT EXISTS idx_providers_state         ON providers(state);
CREATE INDEX IF NOT EXISTS idx_providers_region        ON providers(region);
CREATE INDEX IF NOT EXISTS idx_providers_max_job_ram   ON providers(max_job_ram_gb);
CREATE INDEX IF NOT EXISTS idx_jobs_state              ON jobs(state);
CREATE INDEX IF NOT EXISTS idx_jobs_consumer           ON jobs(consumer_id);
CREATE INDEX IF NOT EXISTS idx_jobs_provider           ON jobs(provider_id);
CREATE INDEX IF NOT EXISTS idx_transactions_account    ON transactions(account_id);
CREATE INDEX IF NOT EXISTS idx_transactions_created_at ON transactions(created_at DESC);

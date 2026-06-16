-- Migration 010: performance indexes + failure_reason on jobs
--
-- idx_escrows_job_id: every complete_job / fail_job / cancel_job fetches the
--   escrow by (job_id, state).  Without this index those are seq-scans.
--
-- idx_transactions_account_created: wallet history queries filter by account_id
--   and sort by created_at DESC — the compound index covers both in one pass.
--
-- failure_reason: stored by fail_job so operators can diagnose failures without
--   parsing logs.  Also written by the heartbeat watcher on provider disconnect.

CREATE INDEX IF NOT EXISTS idx_escrows_job_id
    ON escrows(job_id, state);

CREATE INDEX IF NOT EXISTS idx_transactions_account_created
    ON transactions(account_id, created_at DESC);

ALTER TABLE jobs
    ADD COLUMN IF NOT EXISTS failure_reason TEXT;

-- Backfill last_heartbeat column used by heartbeat watcher (added here if
-- not already present from an earlier ad-hoc migration).
ALTER TABLE jobs
    ADD COLUMN IF NOT EXISTS last_heartbeat TIMESTAMPTZ;

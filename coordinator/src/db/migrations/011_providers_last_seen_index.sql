-- Eviction query scans providers WHERE state = 'available' AND last_seen < threshold.
-- Without this index the query is a full table scan every 60 seconds.
CREATE INDEX IF NOT EXISTS idx_providers_last_seen ON providers(state, last_seen);

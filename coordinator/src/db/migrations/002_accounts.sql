-- Migration 002: device-locked account identity
-- Each account is bound to a specific device via its keypair + hardware fingerprint.
-- account_id = first 24 hex chars of SHA-256(ecdsa_pubkey_spki || device_fingerprint_hash)

CREATE TABLE IF NOT EXISTS accounts (
    -- Primary identity
    account_id              TEXT PRIMARY KEY,

    -- ECDSA P-256 public key in SPKI format, hex-encoded
    -- Used to verify signed API requests
    ecdsa_pubkey_hex        TEXT NOT NULL UNIQUE,

    -- SHA-256(device fingerprint signals) — 64 hex chars
    -- Fingerprint is computed client-side from: userAgent, screen, hardwareConcurrency,
    -- language, timezone, canvas hash. Not secret, just a binding signal.
    device_fingerprint_hash TEXT NOT NULL,

    -- Human-readable device label (optional, set by user)
    device_label            TEXT,

    -- Platform: "macos" | "linux" | "windows" | "browser"
    platform                TEXT DEFAULT 'browser',

    -- Hardware-level serial hash (set by CLI agent; empty for browser accounts)
    -- SHA-256(IOPlatformUUID + serial_number) for macOS
    -- SHA-256(machine-id) for Linux
    hardware_serial_hash    TEXT,

    -- Account role: "consumer" | "provider" | "both"
    role                    TEXT DEFAULT 'consumer',

    -- Whether this account is active
    active                  BOOLEAN DEFAULT TRUE,

    -- Timestamps
    created_at              TIMESTAMPTZ DEFAULT now(),
    last_seen               TIMESTAMPTZ DEFAULT now()
);

-- Separate table for allowed devices per account (multi-device support future)
CREATE TABLE IF NOT EXISTS account_devices (
    id                      BIGSERIAL PRIMARY KEY,
    account_id              TEXT NOT NULL REFERENCES accounts(account_id) ON DELETE CASCADE,
    ecdsa_pubkey_hex        TEXT NOT NULL,
    device_fingerprint_hash TEXT NOT NULL,
    device_label            TEXT,
    platform                TEXT,
    hardware_serial_hash    TEXT,
    is_primary              BOOLEAN DEFAULT FALSE,
    created_at              TIMESTAMPTZ DEFAULT now(),
    UNIQUE(account_id, ecdsa_pubkey_hex)
);

-- Challenge nonces for replay protection on signed requests
CREATE TABLE IF NOT EXISTS auth_nonces (
    nonce       TEXT PRIMARY KEY,
    account_id  TEXT NOT NULL,
    expires_at  TIMESTAMPTZ NOT NULL,
    used        BOOLEAN DEFAULT FALSE
);

-- Index for quick fingerprint lookups
CREATE INDEX IF NOT EXISTS idx_accounts_fingerprint ON accounts(device_fingerprint_hash);
CREATE INDEX IF NOT EXISTS idx_auth_nonces_expiry   ON auth_nonces(expires_at);

-- Purge old nonces periodically (call from cron or on startup)
CREATE OR REPLACE FUNCTION purge_expired_nonces() RETURNS void AS $$
    DELETE FROM auth_nonces WHERE expires_at < now();
$$ LANGUAGE sql;

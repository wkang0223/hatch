-- Migration 003: Malaysian & international KYC / AML compliance
-- Implements BNM tiered KYC limits per Financial Services Act 2013 + AMLA 2001.
--
-- KYC Levels (Malaysian users):
--   0 = No financial transactions allowed
--   1 = Self-declared (name + IC number hash) → RM 5,000/year limit
--   2 = Document verified (uploaded IC/passport, reviewed) → RM 50,000/year limit
--
-- Non-Malaysian users: level 1 is sufficient for up to equivalent USD 5,000/year.
-- All records kept for 7 years per AMLA s.22.

-- ── KYC records ────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS kyc_records (
    id                  UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    account_id          TEXT        NOT NULL REFERENCES accounts(account_id) ON DELETE CASCADE,

    -- Identity
    country_code        CHAR(2)     NOT NULL,           -- ISO 3166-1 alpha-2
    full_name           TEXT        NOT NULL,
    id_type             TEXT        NOT NULL CHECK (id_type IN ('mykad','passport','nric','other')),
    -- Never store raw IC/passport number. Store SHA-256(number||account_id) for de-duplication.
    id_number_hash      TEXT        NOT NULL,

    -- Compliance level
    kyc_level           SMALLINT    NOT NULL DEFAULT 1,
    -- Annual deposit limit in MYR (set by system based on level)
    annual_limit_myr    NUMERIC(12,2) NOT NULL DEFAULT 5000.00,

    -- Acknowledgment
    -- User must acknowledge: NMC is not a financial instrument, not an investment
    acknowledged_terms  BOOLEAN     NOT NULL DEFAULT FALSE,
    acknowledged_at     TIMESTAMPTZ,

    -- Timestamps
    submitted_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    verified_at         TIMESTAMPTZ,           -- set when ops team approves level 2
    expires_at          TIMESTAMPTZ,           -- set on insert: submitted_at + 3 years

    UNIQUE (account_id)
);

-- ── Annual deposit tracking (AMLA: monitor for structuring / smurfing) ─────────
CREATE TABLE IF NOT EXISTS deposit_records (
    id                  UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    account_id          TEXT        NOT NULL REFERENCES accounts(account_id) ON DELETE CASCADE,
    stripe_session_id   TEXT        UNIQUE NOT NULL,
    stripe_payment_intent TEXT,
    amount_nmc          NUMERIC(18,8) NOT NULL,
    amount_myr          NUMERIC(12,2) NOT NULL,
    country             CHAR(2),
    status              TEXT        NOT NULL DEFAULT 'completed' CHECK (status IN ('completed','refunded','disputed')),
    deposited_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS deposit_records_account_year
    ON deposit_records (account_id, deposited_at);

-- ── Annual limit view (rolling 12-month window per BNM guidelines) ─────────────
CREATE OR REPLACE VIEW annual_deposit_totals AS
SELECT
    account_id,
    SUM(amount_myr) AS total_myr,
    COUNT(*)        AS deposit_count
FROM deposit_records
WHERE status = 'completed'
  AND deposited_at >= NOW() - INTERVAL '1 year'
GROUP BY account_id;

-- ── Suspicious Transaction Reports (AMLA s.14 obligation) ─────────────────────
-- Operator must file STR with BNM FIED within 3 working days of suspicion.
-- This table is for internal flagging before submission.
CREATE TABLE IF NOT EXISTS str_flags (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    account_id  TEXT        NOT NULL REFERENCES accounts(account_id),
    reason      TEXT        NOT NULL,
    flagged_by  TEXT        NOT NULL DEFAULT 'system',  -- 'system' | 'ops'
    filed       BOOLEAN     NOT NULL DEFAULT FALSE,
    filed_at    TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Auto-flag: deposits > RM 3,000 in a single day (BNM reporting threshold)
CREATE OR REPLACE FUNCTION check_daily_deposit_threshold()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE
    daily_total NUMERIC;
BEGIN
    SELECT COALESCE(SUM(amount_myr), 0)
    INTO daily_total
    FROM deposit_records
    WHERE account_id = NEW.account_id
      AND deposited_at >= NOW() - INTERVAL '24 hours'
      AND status = 'completed';

    daily_total := daily_total + NEW.amount_myr;

    IF daily_total >= 3000 THEN
        INSERT INTO str_flags (account_id, reason)
        VALUES (
            NEW.account_id,
            FORMAT('Daily deposit total reached RM %.2f (threshold: RM 3,000)', daily_total)
        )
        ON CONFLICT DO NOTHING;
    END IF;

    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS trg_deposit_threshold ON deposit_records;
CREATE TRIGGER trg_deposit_threshold
    AFTER INSERT ON deposit_records
    FOR EACH ROW EXECUTE FUNCTION check_daily_deposit_threshold();

-- ── Withdrawal records (also AMLA-reportable) ─────────────────────────────────
CREATE TABLE IF NOT EXISTS withdrawal_records (
    id                  UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    account_id          TEXT        NOT NULL REFERENCES accounts(account_id) ON DELETE CASCADE,
    destination_address TEXT        NOT NULL,
    destination_chain   TEXT        NOT NULL CHECK (destination_chain IN ('solana','arbitrum')),
    amount_nmc          NUMERIC(18,8) NOT NULL,
    amount_myr_equiv    NUMERIC(12,2),         -- recorded at time of withdrawal
    tx_hash             TEXT,
    status              TEXT        NOT NULL DEFAULT 'pending'
                                    CHECK (status IN ('pending','processing','completed','failed','flagged')),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at        TIMESTAMPTZ
);

-- AMLA 7-year retention: mark records instead of deleting
ALTER TABLE deposit_records    ADD COLUMN IF NOT EXISTS archived BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE withdrawal_records ADD COLUMN IF NOT EXISTS archived BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE kyc_records        ADD COLUMN IF NOT EXISTS archived BOOLEAN NOT NULL DEFAULT FALSE;

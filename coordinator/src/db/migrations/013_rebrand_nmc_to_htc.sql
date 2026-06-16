-- Rebrand: rename all _nmc currency columns to _htc (NeuralMesh Credits → Hatch Token Credits).

ALTER TABLE credit_accounts    RENAME COLUMN available_nmc          TO available_htc;
ALTER TABLE credit_accounts    RENAME COLUMN escrowed_nmc           TO escrowed_htc;
ALTER TABLE credit_accounts    RENAME COLUMN total_earned_nmc       TO total_earned_htc;
ALTER TABLE credit_accounts    RENAME COLUMN total_spent_nmc        TO total_spent_htc;

ALTER TABLE deposit_records    RENAME COLUMN amount_nmc             TO amount_htc;

ALTER TABLE escrows            RENAME COLUMN locked_nmc             TO locked_htc;

ALTER TABLE providers          RENAME COLUMN floor_price_nmc_per_hour TO floor_price_htc_per_hour;

ALTER TABLE transactions       RENAME COLUMN amount_nmc             TO amount_htc;

ALTER TABLE withdrawal_records RENAME COLUMN amount_nmc             TO amount_htc;

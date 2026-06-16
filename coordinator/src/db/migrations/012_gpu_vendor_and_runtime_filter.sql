-- Phase 2: support AMD ROCm (Strix Halo / AI Max+) and NVIDIA Blackwell providers.
-- Adds vendor metadata columns and enables runtime-compatible provider matching.

ALTER TABLE providers ADD COLUMN IF NOT EXISTS gpu_vendor TEXT DEFAULT 'apple';
ALTER TABLE providers ADD COLUMN IF NOT EXISTS compute_api TEXT DEFAULT 'metal';
-- For NVIDIA: "12.0" = Blackwell GB2xx (sm_120), "10.0" = B100/B200 (sm_100)
ALTER TABLE providers ADD COLUMN IF NOT EXISTS cuda_compute_capability TEXT;
-- ROCm version string, e.g. "6.2.0" — required for Strix Halo (gfx1152)
ALTER TABLE providers ADD COLUMN IF NOT EXISTS rocm_version TEXT;
-- GFX target, e.g. "1152" for Strix Halo, "1100" for Navi31
ALTER TABLE providers ADD COLUMN IF NOT EXISTS amd_gfx_target TEXT;
-- True when the AMD GPU uses system RAM as VRAM (Strix Halo, Strix Point)
ALTER TABLE providers ADD COLUMN IF NOT EXISTS is_apu BOOLEAN DEFAULT FALSE;

-- Index for runtime-based provider filtering (matching engine fast path)
CREATE INDEX IF NOT EXISTS idx_providers_runtime
    ON providers USING GIN (installed_runtimes);

-- Update existing Apple Silicon providers to have correct vendor tags
UPDATE providers
SET gpu_vendor  = 'apple',
    compute_api = 'metal'
WHERE gpu_vendor IS NULL OR gpu_vendor = '';

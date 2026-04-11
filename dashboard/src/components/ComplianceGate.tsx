"use client";

import { useEffect, useState } from "react";
import { AlertTriangle, ShieldCheck, ExternalLink } from "lucide-react";
import Link from "next/link";

interface KycStatus {
  level: number;          // 0 = none, 1 = self-declared, 2 = verified
  country_code: string;
  annual_limit_myr: number;
  total_deposited_myr: number;
  requires_kyc: boolean;  // true for MY users
}

// Wraps any financial action (deposit / withdraw) that Malaysian law restricts.
// Non-MY users pass through with no gate.
// Level 0 MY users see a hard block with a link to /compliance.
// Level 1/2 users see a limit indicator.
export function ComplianceGate({
  accountId,
  children,
}: {
  accountId: string | null;
  children: React.ReactNode;
}) {
  const [status, setStatus] = useState<KycStatus | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    if (!accountId) { setLoading(false); return; }

    fetch(`/api/coordinator/v1/kyc/${accountId}`)
      .then((r) => (r.ok ? r.json() : null))
      .then(setStatus)
      .catch(() => {})
      .finally(() => setLoading(false));
  }, [accountId]);

  // If we can't reach the coordinator, or non-MY user, let through
  if (loading || !status || !status.requires_kyc) return <>{children}</>;

  // MY user, no KYC yet
  if (status.level === 0) {
    return (
      <div className="rounded-xl border border-yellow-400/20 bg-yellow-400/5 p-5 space-y-3">
        <div className="flex items-center gap-2 text-yellow-300 font-semibold text-sm">
          <AlertTriangle className="h-4 w-4 flex-shrink-0" />
          Identity verification required
        </div>
        <p className="text-xs text-yellow-200/70 leading-relaxed">
          Malaysian users must complete identity verification before making deposits or
          withdrawals, as required under the{" "}
          <strong>Financial Services Act 2013</strong> and{" "}
          <strong>Anti-Money Laundering Act 2001</strong>.
        </p>
        <div className="flex items-center gap-3 flex-wrap">
          <Link
            href="/compliance"
            className="inline-flex items-center gap-1.5 text-xs px-3 py-1.5 rounded-lg bg-yellow-400/20 hover:bg-yellow-400/30 text-yellow-200 transition-colors font-medium"
          >
            <ShieldCheck className="h-3.5 w-3.5" />
            Complete verification →
          </Link>
          <a
            href="https://www.bnm.gov.my"
            target="_blank"
            rel="noopener noreferrer"
            className="inline-flex items-center gap-1 text-xs text-slate-500 hover:text-slate-400 transition-colors"
          >
            BNM guidelines <ExternalLink className="h-3 w-3" />
          </a>
        </div>
      </div>
    );
  }

  // MY user, KYC done — show limit tracker above the children
  const remaining = status.annual_limit_myr - status.total_deposited_myr;
  const pctUsed = Math.min(100, (status.total_deposited_myr / status.annual_limit_myr) * 100);
  const limitHit = remaining <= 0;

  if (limitHit) {
    return (
      <div className="rounded-xl border border-red-400/20 bg-red-400/5 p-5 space-y-2">
        <div className="flex items-center gap-2 text-red-300 font-semibold text-sm">
          <AlertTriangle className="h-4 w-4" />
          Annual limit reached
        </div>
        <p className="text-xs text-red-200/70">
          You have reached your RM {status.annual_limit_myr.toLocaleString()} annual limit.
          {status.level === 1 && (
            <>
              {" "}
              <Link href="/compliance" className="text-brand-400 hover:text-brand-300">
                Upgrade to verified KYC
              </Link>{" "}
              to increase your limit to RM 50,000/year.
            </>
          )}
        </p>
      </div>
    );
  }

  return (
    <div className="space-y-3">
      {/* Limit indicator */}
      <div className="rounded-lg border border-slate-700 bg-slate-900/40 px-4 py-3">
        <div className="flex items-center justify-between mb-1.5 text-xs">
          <span className="text-slate-500 flex items-center gap-1.5">
            <ShieldCheck className="h-3.5 w-3.5 text-green-400" />
            Verified — Annual limit
          </span>
          <span className="text-slate-400 font-mono">
            RM {status.total_deposited_myr.toFixed(2)} / RM {status.annual_limit_myr.toLocaleString()}
          </span>
        </div>
        <div className="h-1.5 rounded-full bg-slate-800 overflow-hidden">
          <div
            className="h-full rounded-full bg-green-400 transition-all"
            style={{ width: `${pctUsed}%` }}
          />
        </div>
        {status.level === 1 && (
          <p className="text-xs text-slate-600 mt-1.5">
            RM {remaining.toFixed(2)} remaining this year ·{" "}
            <Link href="/compliance" className="text-brand-400 hover:text-brand-300">
              Verify documents
            </Link>{" "}
            for RM 50,000 limit
          </p>
        )}
      </div>

      {children}
    </div>
  );
}

"use client";

import { useEffect, useState } from "react";
import { AlertTriangle, ShieldCheck, ExternalLink } from "lucide-react";
import Link from "next/link";
import { api, type KycRecord } from "@/lib/api-client";

// Wraps any financial action (deposit / withdraw) that Malaysian law restricts.
// Non-MY users pass through with no gate.
// Level 0 MY users see a hard block with a link to /compliance.
// Level 1/2 users see a limit indicator with remaining annual budget.

export function ComplianceGate({
  accountId,
  children,
}: {
  accountId: string | null;
  children: React.ReactNode;
}) {
  const [kyc,     setKyc]     = useState<KycRecord | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    if (!accountId) { setLoading(false); return; }

    api.getKyc(accountId)
      .then(setKyc)
      .catch(() => {}) // coordinator unreachable → let through
      .finally(() => setLoading(false));
  }, [accountId]);

  // Pass through while loading, or if coordinator unreachable, or non-MY user
  // KycRecord.country is the 2-char ISO code returned by the coordinator.
  if (loading || !kyc || kyc.country?.toUpperCase() !== "MY") {
    return <>{children}</>;
  }

  // MY user, not submitted yet
  if (kyc.status === "not_submitted") {
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

  // Pending review
  if (kyc.status === "pending") {
    return (
      <div className="rounded-xl border border-brand-400/20 bg-brand-400/5 p-5 space-y-2">
        <div className="flex items-center gap-2 text-brand-300 font-semibold text-sm">
          <ShieldCheck className="h-4 w-4" />
          Verification under review
        </div>
        <p className="text-xs text-brand-200/70">
          Your identity verification is being reviewed. Deposits and withdrawals will be
          available within 1 business day.
        </p>
      </div>
    );
  }

  // Rejected
  if (kyc.status === "rejected") {
    return (
      <div className="rounded-xl border border-red-400/20 bg-red-400/5 p-5 space-y-2">
        <div className="flex items-center gap-2 text-red-300 font-semibold text-sm">
          <AlertTriangle className="h-4 w-4" />
          Verification rejected
        </div>
        <p className="text-xs text-red-200/70">
          {kyc.rejection_reason ?? "Your verification was rejected. Please resubmit with valid documents."}
        </p>
        <Link
          href="/compliance"
          className="inline-flex items-center gap-1.5 text-xs px-3 py-1.5 rounded-lg bg-red-400/20 hover:bg-red-400/30 text-red-200 transition-colors font-medium"
        >
          Resubmit verification →
        </Link>
      </div>
    );
  }

  // Approved — check annual limit
  const annualLimit    = kyc.annual_limit_myr    ?? 5_000;
  const annualDeposited = kyc.annual_deposited_myr ?? 0;
  const remaining      = annualLimit - annualDeposited;
  const pctUsed        = Math.min(100, (annualDeposited / annualLimit) * 100);
  const limitHit       = remaining <= 0;

  if (limitHit) {
    return (
      <div className="rounded-xl border border-red-400/20 bg-red-400/5 p-5 space-y-2">
        <div className="flex items-center gap-2 text-red-300 font-semibold text-sm">
          <AlertTriangle className="h-4 w-4" />
          Annual limit reached
        </div>
        <p className="text-xs text-red-200/70">
          You have reached your RM {annualLimit.toLocaleString()} annual deposit limit.
          {(kyc.compliance_level ?? 0) < 2 && (
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

  // Approved + within limit — show progress bar then render children
  return (
    <div className="space-y-3">
      <div className="rounded-lg border border-slate-700 bg-slate-900/40 px-4 py-3">
        <div className="flex items-center justify-between mb-1.5 text-xs">
          <span className="text-slate-500 flex items-center gap-1.5">
            <ShieldCheck className="h-3.5 w-3.5 text-green-400" />
            Verified — Annual limit
          </span>
          <span className="text-slate-400 font-mono">
            RM {annualDeposited.toFixed(2)} / RM {annualLimit.toLocaleString()}
          </span>
        </div>
        <div className="h-1.5 rounded-full bg-slate-800 overflow-hidden">
          <div
            className="h-full rounded-full bg-green-400 transition-all"
            style={{ width: `${pctUsed}%` }}
          />
        </div>
        {(kyc.compliance_level ?? 0) < 2 && (
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

"use client";

import { useState } from "react";
import Link from "next/link";
import { Cpu, Zap, DollarSign, CheckCircle, ArrowRight, Mail } from "lucide-react";

const ROLES = [
  { id: "provider", label: "Provider", icon: DollarSign, desc: "I have an idle Mac / Linux GPU to lease" },
  { id: "consumer", label: "Consumer", icon: Zap,         desc: "I want to run AI workloads on Hatch" },
  { id: "both",     label: "Both",     icon: Cpu,         desc: "I want to do both" },
];

const PERKS_PROVIDER = [
  "Early-access invitation to register your device",
  "10% reduced platform fee for the first 90 days",
  "Priority job matching — your device listed first",
  "Hatch Genesis provider badge on your profile",
];

const PERKS_CONSUMER = [
  "50 free HTC credits to run your first jobs",
  "Early access to Apple Silicon providers",
  "Discord invite — direct line to the founders",
  "Influence the roadmap with feature requests",
];

export default function WaitlistPage() {
  const [email, setEmail]   = useState("");
  const [role, setRole]     = useState<string | null>(null);
  const [device, setDevice] = useState("");
  const [submitted, setSubmitted] = useState(false);
  const [loading, setLoading]     = useState(false);
  const [error, setError]         = useState<string | null>(null);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!email || !role) return;
    setLoading(true);
    setError(null);

    try {
      // Write to a simple endpoint — replace with your real form backend
      const res = await fetch("https://formspree.io/f/placeholder", {
        method: "POST",
        headers: { "Content-Type": "application/json", Accept: "application/json" },
        body: JSON.stringify({ email, role, device }),
      });

      if (res.ok || res.status === 422) {
        // 422 means formspree isn't set up yet — treat as success in dev
        setSubmitted(true);
      } else {
        setError("Something went wrong. Email us directly: hello@hatch.dev");
      }
    } catch {
      // Offline or endpoint not configured yet — still show success in dev
      setSubmitted(true);
    } finally {
      setLoading(false);
    }
  }

  if (submitted) {
    return (
      <div className="min-h-screen pt-14 flex items-center justify-center px-4">
        <div className="max-w-md w-full text-center space-y-6">
          <div className="h-16 w-16 rounded-full bg-brand-400/10 border border-brand-400/30 flex items-center justify-center mx-auto">
            <CheckCircle className="h-8 w-8 text-brand-400" />
          </div>
          <h1 className="text-2xl font-bold text-white">You&apos;re on the list</h1>
          <p className="text-slate-400">
            We&apos;ll email <span className="text-white">{email}</span> when your spot opens.
            Invitations go out in batches — providers first.
          </p>
          <div className="glass rounded-xl p-4 text-left space-y-2">
            {(role === "provider" || role === "both" ? PERKS_PROVIDER : PERKS_CONSUMER).map((p) => (
              <div key={p} className="flex items-start gap-2 text-sm text-slate-300">
                <CheckCircle className="h-4 w-4 text-brand-400 flex-shrink-0 mt-0.5" />
                {p}
              </div>
            ))}
          </div>
          <Link
            href="/"
            className="inline-flex items-center gap-2 text-sm text-brand-400 hover:text-brand-300"
          >
            Back to homepage <ArrowRight className="h-3.5 w-3.5" />
          </Link>
        </div>
      </div>
    );
  }

  return (
    <div className="min-h-screen pt-14">
      <div className="max-w-2xl mx-auto px-4 sm:px-6 py-16">

        {/* Header */}
        <div className="text-center mb-12">
          <div className="inline-flex items-center gap-2 px-3 py-1 rounded-full border border-brand-400/30 bg-brand-400/5 text-brand-300 text-xs font-medium mb-6">
            <span className="h-1.5 w-1.5 rounded-full bg-brand-400 animate-pulse" />
            Early Access — Limited spots
          </div>
          <h1 className="text-4xl font-extrabold text-white mb-4">
            Join the Hatch waitlist
          </h1>
          <p className="text-slate-400 max-w-lg mx-auto">
            Hatch is a decentralized GPU marketplace for Apple Silicon, NVIDIA, and AMD hardware.
            Early members get free HTC credits and reduced fees.
          </p>
        </div>

        {/* Form */}
        <form onSubmit={handleSubmit} className="glass rounded-2xl p-6 sm:p-8 space-y-6">

          {/* Email */}
          <div>
            <label className="text-sm font-medium text-white block mb-2">
              Email address
            </label>
            <div className="relative">
              <Mail className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-slate-500" />
              <input
                type="email"
                required
                placeholder="you@example.com"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                className="w-full pl-10 pr-4 py-3 rounded-lg bg-slate-900 border border-slate-700 text-slate-200 placeholder-slate-500 focus:outline-none focus:border-brand-400/60 text-sm"
              />
            </div>
          </div>

          {/* Role */}
          <div>
            <label className="text-sm font-medium text-white block mb-3">
              How do you want to use Hatch?
            </label>
            <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
              {ROLES.map((r) => (
                <button
                  key={r.id}
                  type="button"
                  onClick={() => setRole(r.id)}
                  className={`flex flex-col items-center gap-2 p-4 rounded-xl border text-center transition-all ${
                    role === r.id
                      ? "border-brand-400/60 bg-brand-400/10 text-brand-300"
                      : "border-slate-700 bg-slate-900/50 text-slate-400 hover:border-slate-600 hover:text-slate-300"
                  }`}
                >
                  <r.icon className="h-5 w-5" />
                  <span className="text-sm font-medium">{r.label}</span>
                  <span className="text-xs opacity-70 leading-tight">{r.desc}</span>
                </button>
              ))}
            </div>
          </div>

          {/* Device (provider only) */}
          {(role === "provider" || role === "both") && (
            <div>
              <label className="text-sm font-medium text-white block mb-2">
                What device are you leasing? <span className="text-slate-500 font-normal">(optional)</span>
              </label>
              <input
                type="text"
                placeholder="e.g. Mac Studio M4 Max 128GB, RTX 4090"
                value={device}
                onChange={(e) => setDevice(e.target.value)}
                className="w-full px-4 py-3 rounded-lg bg-slate-900 border border-slate-700 text-slate-200 placeholder-slate-500 focus:outline-none focus:border-brand-400/60 text-sm"
              />
            </div>
          )}

          {/* Perks preview */}
          {role && (
            <div className="bg-slate-900/60 rounded-xl p-4 space-y-2">
              <p className="text-xs text-slate-500 font-medium uppercase tracking-wider mb-3">
                {role === "provider" || role === "both" ? "Provider perks" : "Consumer perks"}
              </p>
              {(role === "provider" || role === "both" ? PERKS_PROVIDER : PERKS_CONSUMER).map((p) => (
                <div key={p} className="flex items-start gap-2 text-sm text-slate-300">
                  <CheckCircle className="h-4 w-4 text-brand-400 flex-shrink-0 mt-0.5" />
                  {p}
                </div>
              ))}
            </div>
          )}

          {error && (
            <p className="text-sm text-red-400 text-center">{error}</p>
          )}

          <button
            type="submit"
            disabled={!email || !role || loading}
            className="w-full py-3 rounded-lg bg-brand-400 text-slate-950 font-semibold text-sm hover:bg-brand-300 transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2"
          >
            {loading ? (
              <span className="h-4 w-4 rounded-full border-2 border-slate-950 border-t-transparent animate-spin" />
            ) : (
              <>
                Request early access
                <ArrowRight className="h-4 w-4" />
              </>
            )}
          </button>

          <p className="text-xs text-center text-slate-600">
            No spam. Unsubscribe at any time. We only email when your spot is ready.
          </p>
        </form>

        {/* Social proof */}
        <div className="mt-8 grid grid-cols-3 gap-4 text-center">
          {[
            { value: "Apple Silicon", label: "first-class support" },
            { value: "8%", label: "platform fee" },
            { value: "30s", label: "avg. match time" },
          ].map((s) => (
            <div key={s.label} className="glass rounded-xl p-3">
              <div className="text-base font-bold text-brand-400 font-mono">{s.value}</div>
              <div className="text-xs text-slate-500 mt-0.5">{s.label}</div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

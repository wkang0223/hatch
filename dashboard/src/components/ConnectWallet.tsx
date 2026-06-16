"use client";

/**
 * ConnectWallet — MetaMask / injected wallet connect button.
 *
 * Shows:
 *   • Disconnected: "Connect Wallet" button
 *   • Connected:    shortened address + network name + disconnect option
 *   • Wrong network: "Switch to Arbitrum" prompt
 */

import { useAccount, useConnect, useDisconnect, useSwitchChain } from "wagmi";
import { arbitrum, arbitrumSepolia }  from "wagmi/chains";
import { cn }                         from "@/lib/utils";
import { Wallet, LogOut, AlertTriangle, Loader2 } from "lucide-react";
import { isMainnet, shortAddr }       from "@/lib/web3";
import { toast }                      from "sonner";

const TARGET_CHAIN = isMainnet ? arbitrum : arbitrumSepolia;

export function ConnectWallet({ className }: { className?: string }) {
  const { address, isConnected, chain } = useAccount();
  const { connect, connectors, isPending } = useConnect();
  const { disconnect } = useDisconnect();
  const { switchChain, isPending: isSwitching } = useSwitchChain();

  const isWrongChain = isConnected && chain?.id !== TARGET_CHAIN.id;

  // ── Disconnected ────────────────────────────────────────────────────────────

  if (!isConnected) {
    const injectedConnector = connectors.find((c) => c.id === "injected");
    return (
      <button
        disabled={isPending || !injectedConnector}
        onClick={() => {
          if (injectedConnector) {
            connect(
              { connector: injectedConnector },
              {
                onError: (e) =>
                  toast.error(`Wallet connection failed: ${e.message}`),
              }
            );
          } else {
            toast.error("No injected wallet found. Install MetaMask or Rabby.");
          }
        }}
        className={cn(
          "flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium",
          "bg-brand-400/15 border border-brand-400/30 text-brand-400",
          "hover:bg-brand-400/25 transition-colors disabled:opacity-50",
          className
        )}
      >
        {isPending ? (
          <Loader2 className="h-4 w-4 animate-spin" />
        ) : (
          <Wallet className="h-4 w-4" />
        )}
        {isPending ? "Connecting…" : "Connect Wallet"}
      </button>
    );
  }

  // ── Wrong chain ─────────────────────────────────────────────────────────────

  if (isWrongChain) {
    return (
      <button
        disabled={isSwitching}
        onClick={() => switchChain({ chainId: TARGET_CHAIN.id })}
        className={cn(
          "flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium",
          "bg-amber-500/10 border border-amber-500/30 text-amber-400",
          "hover:bg-amber-500/20 transition-colors disabled:opacity-50",
          className
        )}
      >
        {isSwitching ? (
          <Loader2 className="h-4 w-4 animate-spin" />
        ) : (
          <AlertTriangle className="h-4 w-4" />
        )}
        {isSwitching ? "Switching…" : `Switch to ${TARGET_CHAIN.name}`}
      </button>
    );
  }

  // ── Connected ───────────────────────────────────────────────────────────────

  return (
    <div className={cn("flex items-center gap-2", className)}>
      <div className="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-slate-800/60 border border-slate-700/50 text-sm">
        <span className="h-2 w-2 rounded-full bg-emerald-400 animate-pulse" />
        <span className="text-slate-300 font-mono">{shortAddr(address ?? "")}</span>
        <span className="text-slate-600">·</span>
        <span className="text-slate-500 text-xs">{chain?.name}</span>
      </div>
      <button
        onClick={() => disconnect()}
        title="Disconnect wallet"
        className="p-1.5 rounded-lg text-slate-500 hover:text-red-400 hover:bg-red-500/10 transition-colors"
      >
        <LogOut className="h-3.5 w-3.5" />
      </button>
    </div>
  );
}

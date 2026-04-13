"use client";

/**
 * Web3Provider — wraps the app with wagmi + TanStack Query context.
 *
 * Must be a client component because wagmi uses browser APIs.
 * Placed in layout.tsx so every page has access to wallet hooks.
 */

import { WagmiProvider }             from "wagmi";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { wagmiConfig }               from "@/lib/web3";
import { useState }                  from "react";

export function Web3Provider({ children }: { children: React.ReactNode }) {
  // Create a stable QueryClient per component lifecycle
  const [queryClient] = useState(() => new QueryClient({
    defaultOptions: {
      queries: {
        // Stale time 30 s — on-chain data doesn't change that fast
        staleTime: 30_000,
        retry: 1,
      },
    },
  }));

  return (
    <WagmiProvider config={wagmiConfig}>
      <QueryClientProvider client={queryClient}>
        {children}
      </QueryClientProvider>
    </WagmiProvider>
  );
}

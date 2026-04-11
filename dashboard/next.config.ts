import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  output: "standalone",
  // Proxy API requests to coordinator and ledger during dev
  async rewrites() {
    return [
      {
        source: "/api/coordinator/:path*",
        destination: `${process.env.COORDINATOR_URL ?? "http://localhost:8080"}/api/:path*`,
      },
      {
        source: "/api/ledger/:path*",
        destination: `${process.env.LEDGER_URL ?? "http://localhost:8082"}/api/:path*`,
      },
    ];
  },
  env: {
    NEXT_PUBLIC_COORDINATOR_URL: process.env.COORDINATOR_URL ?? "http://localhost:8080",
    NEXT_PUBLIC_LEDGER_URL: process.env.LEDGER_URL ?? "http://localhost:8082",
  },
};

export default nextConfig;

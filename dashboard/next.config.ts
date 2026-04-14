import type { NextConfig } from "next";

const COORDINATOR_URL =
  process.env.COORDINATOR_URL ??
  process.env.NEXT_PUBLIC_COORDINATOR_URL ??
  "http://localhost:8080";

const LEDGER_URL =
  process.env.LEDGER_URL ??
  process.env.NEXT_PUBLIC_LEDGER_URL ??
  "http://localhost:8082";

const nextConfig: NextConfig = {
  output: "standalone",
  // Proxy API requests to coordinator and ledger during dev
  async rewrites() {
    return [
      {
        source: "/api/coordinator/:path*",
        destination: `${COORDINATOR_URL}/api/:path*`,
      },
      {
        source: "/api/ledger/:path*",
        destination: `${LEDGER_URL}/api/:path*`,
      },
    ];
  },
  env: {
    NEXT_PUBLIC_COORDINATOR_URL: COORDINATOR_URL,
    NEXT_PUBLIC_LEDGER_URL: LEDGER_URL,
  },
};

export default nextConfig;

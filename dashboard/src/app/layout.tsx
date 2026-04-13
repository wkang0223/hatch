import type { Metadata } from "next";
import "./globals.css";
import { Toaster } from "sonner";
import Navbar from "@/components/Navbar";
import { Web3Provider } from "@/components/Web3Provider";

export const metadata: Metadata = {
  title: "NeuralMesh — Decentralized Apple Silicon GPU Marketplace",
  description:
    "Lease idle Mac Mini M4 Pro and Mac Studio GPU compute for AI inference. " +
    "Run Llama 3 70B on 48GB unified memory at a fraction of cloud GPU costs.",
  keywords: ["apple silicon", "gpu marketplace", "MLX", "AI inference", "Mac Mini", "llm hosting"],
  openGraph: {
    title: "NeuralMesh",
    description: "Decentralized Apple Silicon GPU Marketplace",
    type: "website",
  },
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="en" className="dark">
      <body className="min-h-screen bg-surface-950 text-slate-100 antialiased">
        <Web3Provider>
          <Navbar />
          <main>{children}</main>
        </Web3Provider>
        <Toaster
          position="bottom-right"
          theme="dark"
          toastOptions={{
            style: {
              background: "#0f172a",
              border: "1px solid #334155",
              color: "#e2e8f0",
            },
          }}
        />
      </body>
    </html>
  );
}

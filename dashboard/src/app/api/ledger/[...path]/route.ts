/**
 * Next.js catch-all proxy route: /api/ledger/v1/* → coordinator:8080/api/v1/*
 *
 * The ledger is consolidated onto the coordinator (Phase 1 — no separate ledger
 * service). Browser components that call /api/ledger/v1/... are silently
 * forwarded to the coordinator's /api/v1/... endpoint.
 *
 * This means: /api/ledger/v1/withdraw → coordinator/api/v1/withdraw
 */

import { NextRequest, NextResponse } from "next/server";

const COORDINATOR =
  process.env.COORDINATOR_URL ??
  process.env.NEXT_PUBLIC_COORDINATOR_URL ??
  "http://localhost:8080";

async function proxy(
  req: NextRequest,
  context: { params: Promise<{ path: string[] }> }
): Promise<NextResponse> {
  const { path } = await context.params;
  const targetPath = path.join("/");
  const targetUrl  = `${COORDINATOR}/api/v1/${targetPath}${req.nextUrl.search}`;

  try {
    const headers = new Headers();
    const contentType = req.headers.get("content-type");
    if (contentType) headers.set("content-type", contentType);

    const body =
      req.method !== "GET" && req.method !== "HEAD"
        ? await req.arrayBuffer()
        : undefined;

    const upstream = await fetch(targetUrl, {
      method:  req.method,
      headers,
      body:    body && body.byteLength > 0 ? body : undefined,
      cache:   "no-store",
    });

    const responseBody = await upstream.arrayBuffer();
    const responseHeaders = new Headers();
    const ct = upstream.headers.get("content-type");
    if (ct) responseHeaders.set("content-type", ct);

    return new NextResponse(responseBody, {
      status:  upstream.status,
      headers: responseHeaders,
    });
  } catch (err) {
    console.error(`[ledger-proxy] ${req.method} ${targetUrl} failed:`, err);
    return NextResponse.json(
      { error: "ledger_unavailable", message: "Ledger service is unreachable" },
      { status: 503 }
    );
  }
}

export const GET    = proxy;
export const POST   = proxy;
export const DELETE = proxy;
export const PUT    = proxy;

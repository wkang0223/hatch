/**
 * Next.js catch-all proxy route: /api/coordinator/v1/* → coordinator:8080/api/v1/*
 *
 * Browser components can't call the coordinator directly (different origin,
 * CORS restrictions in production). This proxy:
 *   - Forwards GET/POST/DELETE methods
 *   - Preserves request body and Content-Type
 *   - Streams the response back
 *   - Adds no authentication (coordinator handles its own security)
 *
 * Example:
 *   Browser fetches /api/coordinator/v1/stats
 *   → This route forwards to http://localhost:8080/api/v1/stats
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
      // Don't cache — coordinator data is real-time
      cache:   "no-store",
    });

    const responseBody = await upstream.arrayBuffer();
    const responseHeaders = new Headers();
    const upstreamContentType = upstream.headers.get("content-type");
    if (upstreamContentType) {
      responseHeaders.set("content-type", upstreamContentType);
    }

    return new NextResponse(responseBody, {
      status:  upstream.status,
      headers: responseHeaders,
    });
  } catch (err) {
    // Coordinator unreachable — return a structured error so UI can handle gracefully
    console.error(`[coordinator-proxy] ${req.method} ${targetUrl} failed:`, err);
    return NextResponse.json(
      { error: "coordinator_unavailable", message: "Backend service is unreachable" },
      { status: 503 }
    );
  }
}

export const GET    = proxy;
export const POST   = proxy;
export const DELETE = proxy;
export const PUT    = proxy;
export const PATCH  = proxy;

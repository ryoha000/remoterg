/**
 * Cloudflare Workers エントリーポイント
 *
 * WebRTCシグナリング用のDurable Objectをホストする独立Worker
 */

import { SignalingSession as Impl } from "./signaling/session";
export class SignalingSession extends Impl {}

export default {
  async fetch(
    request: Request,
    env: { SIGNALING_SESSION: DurableObjectNamespace },
    _ctx?: ExecutionContext
  ): Promise<Response> {
    const url = new URL(request.url);

    // WebSocketシグナリングリクエストをDurable Objectへルーティング
    if (url.pathname === "/api/signal" && request.headers.get("Upgrade") === "websocket") {
      const sessionId = url.searchParams.get("session_id") || "fixed";
      const role = url.searchParams.get("role");

      if (!role || (role !== "host" && role !== "viewer")) {
        return new Response(
          'Invalid role parameter. Must be "host" or "viewer"',
          { status: 400 }
        );
      }

      const id = env.SIGNALING_SESSION.idFromName(sessionId);
      const stub = env.SIGNALING_SESSION.get(id);
      return stub.fetch(request);
    }

    return new Response("Not Found", { status: 404 });
  },
};

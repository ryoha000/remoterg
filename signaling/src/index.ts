/**
 * Cloudflare Workers エントリーポイント
 *
 * WebRTCシグナリング用のDurable Objectをホストする独立Worker
 */

import { SignalingSession as Impl } from "./signaling/session";
import { getAuthUrl, exchangeCodeForTokens } from "./drive/oauth";

export class SignalingSession extends Impl {}

export interface Env {
  SIGNALING_SESSION: DurableObjectNamespace;
  GOOGLE_CLIENT_ID: string;
  GOOGLE_CLIENT_SECRET: string;
  GOOGLE_REDIRECT_URI?: string;
}

export default {
  async fetch(
    request: Request,
    env: Env,
    _ctx?: ExecutionContext
  ): Promise<Response> {
    const url = new URL(request.url);

    // Google Drive Auth ルーティング
    if (url.pathname === "/api/drive/auth-url") {
      const clientId = env.GOOGLE_CLIENT_ID;
      // デフォルトは Android アプリのカスタム URI スキーム
      const redirectUri = env.GOOGLE_REDIRECT_URI || "moe.ryoha.remoterg:/oauth2callback";
      if (!clientId) {
        return new Response("Missing GOOGLE_CLIENT_ID", { status: 500 });
      }
      const authUrl = getAuthUrl(clientId, redirectUri);
      return new Response(JSON.stringify({ url: authUrl }), {
        headers: {
          "Content-Type": "application/json",
          "Access-Control-Allow-Origin": "*",
        },
      });
    }

    if (url.pathname === "/api/drive/token" && request.method === "POST") {
      const clientId = env.GOOGLE_CLIENT_ID;
      const redirectUri = env.GOOGLE_REDIRECT_URI || "moe.ryoha.remoterg:/oauth2callback";

      if (!clientId) {
        return new Response("Missing OAuth credentials in env", { status: 500 });
      }

      try {
        const body = (await request.json()) as { code: string };
        if (!body.code) {
          return new Response("Missing code in request body", { status: 400 });
        }

        const tokens = await exchangeCodeForTokens(
          clientId,
          redirectUri,
          body.code
        );
        return new Response(JSON.stringify(tokens), {
          headers: {
            "Content-Type": "application/json",
            "Access-Control-Allow-Origin": "*",
          },
        });
      } catch (e: any) {
        return new Response(JSON.stringify({ error: e.message }), {
          status: 500,
          headers: {
            "Content-Type": "application/json",
            "Access-Control-Allow-Origin": "*",
          },
        });
      }
    }

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

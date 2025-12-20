import { createFileRoute } from "@tanstack/react-router";
import { env } from "cloudflare:workers";

export const Route = createFileRoute("/api/signal")({
  server: {
    handlers: {
      GET: async ({ request }) => {
        const url = new URL(request.url);
        const sessionId = url.searchParams.get("session_id") || "fixed";
        const role = url.searchParams.get("role");

        if (!role || (role !== "host" && role !== "viewer")) {
          return new Response(
            'Invalid role parameter. Must be "host" or "viewer"',
            { status: 400 }
          );
        }

        // Cloudflare環境からDurable Objectを取得
        // @cloudflare/vite-pluginを使用している場合、import { env } from "cloudflare:workers"で
        // バインディングに直接アクセスできます
        if (!env.SIGNALING_SESSION) {
          console.error(
            "SIGNALING_SESSION Durable Object not found in environment"
          );
          return new Response("Durable Objects not configured", {
            status: 500,
          });
        }

        // Durable Object IDを生成（session_idから）
        const id = env.SIGNALING_SESSION.idFromName(sessionId);
        const stub = env.SIGNALING_SESSION.get(id);

        console.log("stub", stub);

        // Durable Objectにリクエストを転送
        const doRequest = new Request(request.url, {
          headers: request.headers,
        });

        return stub.fetch(doRequest);
      },
    },
  },
});

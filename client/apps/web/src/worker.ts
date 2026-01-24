/**
 * Cloudflare Workers entry point
 *
 * This file exports the Durable Object class and the fetch handler
 * for Cloudflare Workers.
 */

import { SignalingSession as Impl } from "./signaling/session";
export class SignalingSession extends Impl {}

// Cloudflare Workers用のfetchハンドラーをエクスポート
// @cloudflare/vite-pluginは、worker.tsからdefaultエクスポートを期待しており、
// そのエクスポートがfetchメソッドを持つオブジェクトである必要があります。
export default {
  async fetch(
    request: Request,
    env: any,
    _ctx?: ExecutionContext
  ): Promise<Response> {
    // TanStack Startのサーバーエントリーポイントを動的にインポート
    const serverEntry = await import("@tanstack/react-start/server-entry");

    // TanStack Startのサーバーエントリーポイントを使ってリクエストを処理
    // @cloudflare/vite-pluginにより、envは"cloudflare:workers"から直接インポート可能
    return serverEntry.default.fetch(request, env);
  },
};

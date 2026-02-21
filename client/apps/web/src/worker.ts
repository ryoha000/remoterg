/**
 * Cloudflare Workers entry point
 *
 * TanStack Startのサーバーエントリーポイントのみを処理する
 * シグナリングは独立した signaling Worker で動作する
 */

export default {
  async fetch(
    request: Request,
    env: unknown,
    _ctx?: ExecutionContext
  ): Promise<Response> {
    // TanStack Startのサーバーエントリーポイントを動的にインポート
    const serverEntry = await import("@tanstack/react-start/server-entry");

    // TanStack Startのサーバーエントリーポイントを使ってリクエストを処理
    return serverEntry.default.fetch(request, env);
  },
};

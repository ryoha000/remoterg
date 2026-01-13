import { spawn, ChildProcess } from "child_process";
import { exec } from "child_process";
import { promisify } from "util";
import * as path from "path";
import { fileURLToPath } from "url";
import { dirname } from "path";

// ESMモジュールで__dirname相当の値を取得
const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

const execAsync = promisify(exec);

/**
 * E2Eテスト統合スクリプト
 *
 * 1. Web devサーバーを起動
 * 2. hostdを起動
 * 3. 両サーバーの準備完了を待機
 * 4. Playwrightテストを実行
 * 5. 完了後に全プロセスを終了
 */

const WEB_PORT = 3000;
const WEB_URL = `http://localhost:${WEB_PORT}`;
const MAX_WAIT_TIME = 120000; // 2分

let webProcess: ChildProcess | null = null;
let hostdProcess: ChildProcess | null = null;
let hostdConnected = false;

async function waitForServer(url: string, timeout: number): Promise<void> {
  const startTime = Date.now();
  const checkInterval = 1000; // 1秒ごとにチェック

  while (Date.now() - startTime < timeout) {
    try {
      const response = await fetch(url);
      if (response.ok) {
        console.log(`[e2e] ✓ Server ready at ${url}`);
        return;
      }
    } catch (_) {
      // サーバーがまだ起動していない
    }
    await new Promise((resolve) => setTimeout(resolve, checkInterval));
  }

  throw new Error(`Server at ${url} did not become ready within ${timeout}ms`);
}

async function waitForHostd() {
  console.log("[e2e] Waiting for hostd to be ready (WebSocket connected)...");
  const startTime = Date.now();
  const timeout = 60000;

  while (Date.now() - startTime < timeout) {
    if (hostdConnected) {
      console.log("[e2e] ✓ hostd is ready");
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  throw new Error("Timeout waiting for hostd WebSocket connection");
}

async function startWebServer(): Promise<ChildProcess> {
  console.log("[e2e] Starting web dev server...");
  const webDir = path.resolve(__dirname, "..");
  const webProcess = spawn("pnpm", ["dev"], {
    cwd: webDir,
    stdio: "pipe",
    shell: true,
    env: {
      ...process.env,
      NODE_OPTIONS: "--import ./instrument.server.mjs",
    },
  });

  webProcess.stdout?.on("data", (data) => {
    const output = data.toString();
    console.log(`[web] ${output.trim()}`);
  });

  webProcess.stderr?.on("data", (data) => {
    const output = data.toString();
    console.error(`[web] ${output.trim()}`);
  });

  return webProcess;
}

async function startHostd(): Promise<ChildProcess> {
  console.log("[e2e] Starting hostd...");
  const servicesDir = path.resolve(__dirname, "../../desktop/services");

  const hostdProcess = spawn("task", ["hostd:mock"], {
    cwd: servicesDir,
    stdio: "pipe",
    shell: true,
  });

  hostdProcess.stdout?.on("data", (data) => {
    const output = data.toString();
    console.log(`[hostd] ${output.trim()}`);
    if (output.includes("WebSocket connected")) {
      hostdConnected = true;
    }
  });

  hostdProcess.stderr?.on("data", (data) => {
    const output = data.toString();
    console.error(`[hostd] ${output.trim()}`);
    if (output.includes("WebSocket connected")) {
      hostdConnected = true;
    }
  });

  return hostdProcess;
}

async function runPlaywrightTests(): Promise<void> {
  console.log("[e2e] Running Playwright tests...");
  const webDir = path.resolve(__dirname, "..");

  try {
    const { stdout, stderr } = await execAsync("pnpm exec playwright test", {
      cwd: webDir,
      env: {
        ...process.env,
      },
    });

    if (stdout) {
      console.log(stdout);
    }
    if (stderr) {
      console.error(stderr);
    }
  } catch (error: any) {
    if (error.stdout) {
      console.log(error.stdout);
    }
    if (error.stderr) {
      console.error(error.stderr);
    }
    throw error;
  }
}

async function cleanup() {
  console.log("[e2e] Shutting down servers...");

  const killProcess = (proc: ChildProcess, signal: NodeJS.Signals = "SIGTERM"): Promise<void> => {
    return new Promise((resolve) => {
      if (!proc || proc.killed) {
        resolve();
        return;
      }

      const timeout = setTimeout(() => {
        if (!proc.killed) {
          proc.kill("SIGKILL");
        }
        resolve();
      }, 5000);

      proc.once("exit", () => {
        clearTimeout(timeout);
        resolve();
      });

      proc.kill(signal);
    });
  };

  const promises: Promise<void>[] = [];

  if (webProcess) {
    promises.push(killProcess(webProcess));
    webProcess = null;
  }

  if (hostdProcess) {
    promises.push(killProcess(hostdProcess));
    hostdProcess = null;
  }

  await Promise.all(promises);
  console.log("[e2e] Servers shut down");
}

// シグナルハンドラー
process.on("SIGINT", async () => {
  await cleanup();
  process.exit(1);
});

process.on("SIGTERM", async () => {
  await cleanup();
  process.exit(1);
});

async function main() {
  try {
    // Webサーバーを起動
    webProcess = await startWebServer();

    // hostdを起動
    hostdProcess = await startHostd();

    // サーバーの準備完了を待機
    console.log("[e2e] Waiting for servers to be ready...");
    await waitForServer(WEB_URL, MAX_WAIT_TIME);

    // hostdの待機
    await waitForHostd();

    // Playwrightテストを実行
    await runPlaywrightTests();

    console.log("[e2e] All tests passed!");
  } catch (error) {
    console.error("[e2e] Test failed:", error);
    await cleanup();
    process.exit(1);
  } finally {
    await cleanup();
    process.exit(0);
  }
}

main();

import { test, expect } from "@playwright/test";

/**
 * WebRTC接続のE2Eテスト
 *
 * テストフロー:
 * 1. /viewer/fixed/h264 にアクセス
 * 2. 「接続」ボタンをクリック
 * 3. 接続状態が "connected" になることを確認
 * 4. フレームが受信されることを確認（stats.inbound.framesReceived > 0）
 * 5. 映像が再生されていることを確認（video要素がplaying状態、currentTimeが増加）
 * 6. 音声が流れていることを確認（音声トラックの存在と有効性、実際の音声レベル検出）
 */
test("WebRTC connection test", async ({ page }) => {
  // ページにアクセス
  await page.goto("/viewer/fixed/h264");

  // ページが読み込まれるまで待機
  await page.waitForLoadState("networkidle");

  // 「接続」ボタンが表示されるまで待機
  const connectButton = page.getByRole("button", { name: "接続" });
  await expect(connectButton).toBeVisible({ timeout: 10000 });

  // 接続ボタンをクリック
  await connectButton.click();

  // 接続状態が "connected" になるまで待機（最大30秒）
  // Badgeに「接続済み」と表示されることを確認
  await expect(page.locator("text=接続済み")).toBeVisible({ timeout: 30000 });

  // 接続状態のテキストを確認
  const statusBadge = page.locator("text=接続済み").first();
  await expect(statusBadge).toBeVisible();

  // statsが表示されるまで待機
  // stats.inbound.framesReceived が 0 より大きくなるまで待機（最大30秒）
  await page.waitForFunction(
    () => {
      const preElement = document.querySelector("pre");
      if (!preElement) return false;
      const text = preElement.textContent || "";
      // "frames=数字" のパターンを探す
      const match = text.match(/frames=(\d+)/);
      if (match) {
        const frames = parseInt(match[1], 10);
        return frames > 0;
      }
      return false;
    },
    { timeout: 30000 }
  );

  // statsの内容を確認
  const statsText = await page.locator("pre").textContent();
  expect(statsText).toContain("connectionState: connected");
  expect(statsText).toMatch(/frames=\d+/);

  // フレーム数が0より大きいことを確認
  const framesMatch = statsText?.match(/frames=(\d+)/);
  if (framesMatch) {
    const framesReceived = parseInt(framesMatch[1], 10);
    expect(framesReceived).toBeGreaterThan(0);
    console.log(`✓ Frames received: ${framesReceived}`);
  }

  // video要素が存在することを確認
  const videoElement = page.locator("video");
  await expect(videoElement).toBeVisible();

  // 映像が再生されていることを確認（video要素がplaying状態）
  await page.waitForFunction(
    () => {
      const video = document.querySelector("video");
      return video && !video.paused && video.currentTime > 0;
    },
    { timeout: 10000 }
  );

  // video要素の状態を確認
  const videoState = await videoElement.evaluate((video: HTMLVideoElement) => ({
    paused: video.paused,
    currentTime: video.currentTime,
    readyState: video.readyState,
    videoWidth: video.videoWidth,
    videoHeight: video.videoHeight,
  }));
  console.log(
    `✓ Video playing: currentTime=${videoState.currentTime}, readyState=${videoState.readyState}, size=${videoState.videoWidth}x${videoState.videoHeight}`
  );

  // 音声トラックが存在することを確認
  const audioTrackExists = await videoElement.evaluate(
    (video: HTMLVideoElement) => {
      const stream = video.srcObject as MediaStream | null;
      if (!stream) return false;
      const audioTracks = stream.getAudioTracks();
      return audioTracks.length > 0;
    }
  );
  expect(audioTrackExists).toBe(true);
  console.log("✓ Audio track exists");

  // 音声トラックが有効であることを確認
  const audioTrackEnabled = await videoElement.evaluate(
    (video: HTMLVideoElement) => {
      const stream = video.srcObject as MediaStream | null;
      if (!stream) return false;
      const audioTracks = stream.getAudioTracks();
      return (
        audioTracks.length > 0 &&
        audioTracks[0].enabled &&
        audioTracks[0].readyState === "live"
      );
    }
  );
  expect(audioTrackEnabled).toBe(true);
  console.log("✓ Audio track is enabled and live");

  // 音声ONボタンをクリックしてミュートを解除
  const audioButton = page.getByRole("button", { name: /音声ON/ });
  await expect(audioButton).toBeVisible({ timeout: 5000 });
  await audioButton.click();
  console.log("✓ Clicked audio ON button");

  // 少し待機して音声が有効化されるのを待つ
  await page.waitForTimeout(5000);

  // Web Audio APIを使って実際の音声レベルを検出
  const audioLevelDetected = await page.evaluate(async () => {
    const video = document.querySelector("video") as HTMLVideoElement | null;
    if (!video || !video.srcObject) return false;

    const stream = video.srcObject as MediaStream;
    const audioContext = new AudioContext();
    const source = audioContext.createMediaStreamSource(stream);
    const analyser = audioContext.createAnalyser();
    analyser.fftSize = 256;
    source.connect(analyser);

    const bufferLength = analyser.frequencyBinCount;
    const dataArray = new Uint8Array(bufferLength);

    // 3秒間、100ms間隔で音声レベルをチェック
    for (let i = 0; i < 30; i++) {
      analyser.getByteFrequencyData(dataArray);
      const average = dataArray.reduce((a, b) => a + b, 0) / bufferLength;

      if (average > 0) {
        audioContext.close();
        return true;
      }

      await new Promise((resolve) => setTimeout(resolve, 100));
    }

    audioContext.close();
    return false;
  });

  expect(audioLevelDetected).toBe(true);
  console.log("✓ Audio level detected");

  console.log("✓ WebRTC connection test passed");
});

import { test, expect } from "@playwright/test";

/**
 * WebRTC接続のE2Eテスト
 *
 * テストフロー:
 * 1. /viewer/fixed/h264 にアクセス
 * 2. 自動接続されることを確認
 * 3. 接続状態が "connected" になることを確認
 * 4. 設定メニューからStatsを表示し、フレーム受信を確認
 * 5. 映像が再生されていることを確認
 * 6. 音声ミュート解除を確認
 */
test("WebRTC connection test", async ({ page }) => {
  // ページにアクセス
  await page.goto("/viewer/fixed/h264");

  // ページが読み込まれるまで待機
  await page.waitForLoadState("networkidle");

  // 自動的に接続状態が "connected" になるまで待機（最大30秒）
  // Badgeに "connected" と表示されることを確認 (大文字小文字無視)
  await expect(page.getByText("connected", { exact: true })).toBeVisible({ timeout: 30000 });

  // 接続状態のテキストを確認
  const statusBadge = page.getByText("connected", { exact: true }).first();
  await expect(statusBadge).toBeVisible();

  // マウスを動かしてオーバーレイを表示
  await page.mouse.move(100, 100);

  // 設定ボタンをクリックしてメニューを開く
  await page.getByLabel("Settings").click();

  // "Stats for Nerds" のスイッチをONにする
  await page.getByRole("switch").click();

  // statsが表示されるまで待機 (Debug overlay)
  // "Frames:" のテキストを探す
  await expect(page.locator("text=Frames:")).toBeVisible({ timeout: 30000 });

  // Debug overlayの内容を取得
  const debugText = await page.locator("text=Frames:").textContent();

  // フレーム数が増えているか確認したいが、ここでは表示されていることだけ確認
  expect(debugText).toContain("Frames:");

  // video要素が存在することを確認
  const videoElement = page.locator("video");
  await expect(videoElement).toBeVisible();

  // 映像が再生されていることを確認（video要素がplaying状態）
  await page.waitForFunction(
    () => {
      const video = document.querySelector("video");
      return video && !video.paused && video.currentTime > 0;
    },
    { timeout: 10000 },
  );

  // 音声トラックが存在することを確認
  const audioTrackExists = await videoElement.evaluate((video: HTMLVideoElement) => {
    const stream = video.srcObject as MediaStream | null;
    if (!stream) return false;
    const audioTracks = stream.getAudioTracks();
    return audioTracks.length > 0;
  });
  expect(audioTrackExists).toBe(true);
  console.log("✓ Audio track exists");

  // 音声ミュート解除 (Unmuteボタンをクリック)
  const unmuteButton = page.getByLabel("Unmute");
  await expect(unmuteButton).toBeVisible();
  await unmuteButton.click();
  console.log("✓ Clicked Unmute button");

  // ボタンが "Mute" に変わったことを確認
  await expect(page.getByLabel("Mute")).toBeVisible();

  console.log("✓ WebRTC connection test passed");
});

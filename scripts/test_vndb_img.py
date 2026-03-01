"""段階的テスト: llama-server の画像処理制限を特定"""
import requests
import base64

LLAMA_URL = "http://127.0.0.1:8081/v1/chat/completions"
SCREENSHOT_PATH = r"F:\workspace\remoterg\desktop\services\assets\screenshots\9c6cf48d-d2b4-46e8-bc58-a86257a552b2.jpeg"

def detect_mime(data: bytes) -> str:
    if len(data) >= 2 and data[0] == 0xFF and data[1] == 0xD8:
        return "image/jpeg"
    elif len(data) >= 4 and data[:4] == b"\x89PNG":
        return "image/png"
    return "image/jpeg"

# VNDBからキャラクター画像を取得
body = {
    "filters": ["vn", "=", ["id", "=", "v60196"]],
    "fields": "name, original, image.url",
    "sort": "name",
    "results": 8,
    "page": 1,
}
resp = requests.post("https://api.vndb.org/kana/character", json=body)
chars = resp.json()["results"]

# 全画像ダウンロード
all_images = []
for c in chars:
    img = c.get("image")
    url = img.get("url") if img else None
    if not url:
        continue
    name = c.get("original") or c.get("name")
    img_resp = requests.get(url)
    if img_resp.status_code == 200:
        all_images.append((name, img_resp.content))
        print(f"Downloaded: {name} ({len(img_resp.content)} bytes, mime={detect_mime(img_resp.content)})")

with open(SCREENSHOT_PATH, "rb") as f:
    screenshot_data = f.read()

screenshot_mime = detect_mime(screenshot_data)
print(f"\nスクリーンショット: {len(screenshot_data)} bytes, mime={screenshot_mime}")

def test_request(label, content_parts):
    llm_body = {
        "messages": [{"role": "user", "content": content_parts}],
        "max_tokens": 16,
        "temperature": 0.7,
        "stream": False,
    }
    try:
        r = requests.post(LLAMA_URL, json=llm_body, timeout=120)
        if r.status_code == 200:
            usage = r.json().get("usage", {})
            print(f"  [{label}] OK - tokens: {usage.get('prompt_tokens')}")
        else:
            err = r.json().get("error", {})
            print(f"  [{label}] FAIL {r.status_code} - {err.get('message', r.text[:200])}")
    except Exception as e:
        print(f"  [{label}] ERROR - {e}")

# テスト1: キャラ画像1枚のみ（スクリーンショットなし）
print("\n=== テスト1: キャラ画像1枚のみ ===")
parts = [
    {"type": "text", "text": "Describe"},
    {"type": "image_url", "image_url": {"url": f"data:image/jpeg;base64,{base64.b64encode(all_images[0][1]).decode()}"}}
]
test_request("char_1_only", parts)

# テスト2: キャラ画像1枚 + スクリーンショット1枚
print("\n=== テスト2: キャラ1 + スクリーンショット ===")
parts = [
    {"type": "text", "text": "Describe"},
    {"type": "image_url", "image_url": {"url": f"data:image/jpeg;base64,{base64.b64encode(all_images[0][1]).decode()}"}},
    {"type": "text", "text": "Screenshot:"},
    {"type": "image_url", "image_url": {"url": f"data:{screenshot_mime};base64,{base64.b64encode(screenshot_data).decode()}"}}
]
test_request("char_1_ss", parts)

# テスト3: キャラ画像2枚（スクリーンショットなし）
print("\n=== テスト3: キャラ画像2枚のみ ===")
parts = [{"type": "text", "text": "Describe"}]
for name, img_data in all_images[:2]:
    parts.append({"type": "image_url", "image_url": {"url": f"data:image/jpeg;base64,{base64.b64encode(img_data).decode()}"}})
test_request("char_2_only", parts)

# テスト4: キャラ画像2枚 + スクリーンショット
print("\n=== テスト4: キャラ2 + スクリーンショット ===")
parts = [{"type": "text", "text": "Describe"}]
for name, img_data in all_images[:2]:
    parts.append({"type": "image_url", "image_url": {"url": f"data:image/jpeg;base64,{base64.b64encode(img_data).decode()}"}})
parts.append({"type": "image_url", "image_url": {"url": f"data:{screenshot_mime};base64,{base64.b64encode(screenshot_data).decode()}"}})
test_request("char_2_ss", parts)

# テスト5: スクリーンショット1枚のみ
print("\n=== テスト5: スクリーンショットのみ ===")
parts = [
    {"type": "text", "text": "Describe"},
    {"type": "image_url", "image_url": {"url": f"data:{screenshot_mime};base64,{base64.b64encode(screenshot_data).decode()}"}}
]
test_request("ss_only", parts)

# テスト6: スクリーンショット2枚（同じ画像）
print("\n=== テスト6: スクリーンショット2枚 ===")
parts = [
    {"type": "text", "text": "Describe"},
    {"type": "image_url", "image_url": {"url": f"data:{screenshot_mime};base64,{base64.b64encode(screenshot_data).decode()}"}},
    {"type": "image_url", "image_url": {"url": f"data:{screenshot_mime};base64,{base64.b64encode(screenshot_data).decode()}"}}
]
test_request("ss_2", parts)

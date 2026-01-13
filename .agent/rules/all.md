---
trigger: always_on
---

返答は日本語で行うこと
plan や walkthrough は日本語で書くこと
task e2e は実行に時間がかかるため、実行する際は適当にそのログをファイルに書き出しコードの変更がない場合にログを確認したいときはそのログファイルを参照してください

## Project Overview

RemoteRG is a remote play application for visual novels that streams a Windows game window to smartphones/tablets over WebRTC with synchronized input control via DataChannel.

- **Backend**: Rust-based host daemon (`hostd`) managing video capture, encoding, WebRTC, and input
- **Frontend**: TanStack Start/React web client deployed to Cloudflare Workers

## Build & Development Commands

### Rust Backend (desktop/services)

```bash
cargo build --release                    # Build
cargo run --bin hostd                    # Run with defaults
cargo run --bin hostd -- --port 8080 --log-level debug  # Run with options
cargo run --bin hostd -- mock    # Run with dummy frames (testing)
cargo check                              # Verify compilation
cargo test                               # Run tests
cargo bench --package encoder            # Benchmark encoder
```

### Web Client (web/)

```bash
pnpm install          # Install dependencies
pnpm dev              # Development server (port 3000)
pnpm build            # Production build
pnpm lint             # Lint with oxlint
pnpm lint:fix         # Fix lint issues
pnpm fmt              # Format with oxfmt
pnpm deploy           # Deploy to Cloudflare
pnpm dlx shadcn@latest add <component>  # Add shadcn component
```

### Task Runner (Taskfile.yml)

```bash
task hostd            # Run hostd locally
task hostd:remote     # Run hostd with Cloudflare signaling
task web              # Start web dev server
task e2e              # Run e2e tests
```

## Architecture

### Service Structure

Services communicate via tokio channels with no cross-service dependencies. Only `hostd` orchestrates services.

```
Browser ──WebSocket──> Cloudflare Worker/DO ──WebSocket──> hostd
                                                            │
                    ┌───────────────────────────────────────┘
                    ↓
   ┌─────────────────────────────────────────────────────────┐
   │ hostd (orchestrator)                                    │
   │  ├─ CaptureService → Frame → EncoderService → H.264    │
   │  ├─ WebRtcService ← channels → VideoTrack → Browser    │
   │  ├─ SignalingClient ← WebSocket → Cloudflare DO        │
   │  └─ InputService ← DataChannel messages                 │
   └─────────────────────────────────────────────────────────┘
```

### Crate Dependencies (strict hierarchy)

- **core-types**: Shared DTOs (Frame, WebRtcMessage, etc.) - depends on nothing
- **capture, encoder, webrtc, signaling, input**: Each depends only on core-types
- **hostd**: Orchestrator, depends on all services

### Key Files

- `desktop/services/core/src/lib.rs` - Shared types for all services
- `desktop/services/hostd/src/main.rs` - Service orchestration and channel wiring
- `desktop/services/webrtc/src/connection.rs` - PeerConnection handling
- `web/src/routes/` - TanStack Router file-based routes

## Development Rules

### Rust

- After modifying Rust code, run `cargo check` to verify compilation before finishing
- Shared types between services must be added to `core-types` crate
- Services must not depend on each other directly - only on core-types
- Encoder factory injection is done in hostd (feature flags: h264)

### Web

- Sentry error tracking is configured in `src/router.tsx`
- Wrap server functions with `Sentry.startSpan()` for instrumentation
- Use latest shadcn CLI for components: `pnpm dlx shadcn@latest add <name>`

## Tech Stack

### Backend

- tokio 1.40, webrtc-rs 0.14, windows-capture 2.0-alpha
- Media Foundation H.264 (hardware), OpenH264 (fallback)
- tokio-tungstenite for WebSocket client

### Frontend

- TanStack Start 1.132, React 19.2, Tailwind CSS 4
- Deployed to Cloudflare Workers with Durable Objects for signaling
- The use of the "any" type is not permitted.

## Documentation

- `SPEC.md` - Product specification (Japanese)

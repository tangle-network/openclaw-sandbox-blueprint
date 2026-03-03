# Migration: JS Scaffold → Rust Blueprint

## What changed

The original JS-centric scaffold has been replaced with a blueprint-sdk-native
Rust implementation. The product intent (hosted OpenClaw instance orchestration)
is preserved, but the implementation path is now Rust.

## Mapping

| JS scaffold | Rust blueprint |
|---|---|
| `package.json` + `npm start` | `Cargo.toml` workspace + `cargo run` |
| `openclaw-hosting-blueprint-lib/src/jobs/lifecycleJobs.js` | `openclaw-hosting-blueprint-lib/src/jobs/lifecycle.rs` |
| `openclaw-hosting-blueprint-lib/src/services/hostedInstanceService.js` | `openclaw-hosting-blueprint-lib/src/state.rs` |
| `openclaw-hosting-blueprint-lib/src/runtime/mockSandboxRuntimeAdapter.js` | Adapter boundary in lib crate (to be wired) |
| `openclaw-hosting-blueprint-lib/src/server/createHttpService.js` | Operator HTTP API (planned, not yet wired) |
| `openclaw-hosting-blueprint-lib/src/server/bootstrap.js` | `openclaw-hosting-blueprint-bin/src/main.rs` |
| `openclaw-hosting-blueprint-lib/src/config/templateLoader.js` | Template loading (planned, packs retained in config/) |
| `openclaw-hosting-blueprint-lib/bin/dev-server.js` | `cargo run --bin openclaw-hosting-blueprint` |
| `openclaw-hosting-blueprint-lib/bin/smoke-test.js` | `cargo test --all` |
| HTTP POST `/jobs/create-hosted-instance` | On-chain job ID 0 (`create_instance`) |
| HTTP POST `/jobs/start-hosted-instance` | On-chain job ID 1 (`start_instance`) |
| HTTP POST `/jobs/stop-hosted-instance` | On-chain job ID 2 (`stop_instance`) |
| HTTP POST `/jobs/delete-hosted-instance` | On-chain job ID 3 (`delete_instance`) |
| HTTP GET `/instances` | Operator HTTP API query (planned) |
| HTTP GET `/instances/:id` | Operator HTTP API query (planned) |
| HTTP GET `/templates` | Operator HTTP API query (planned) |

## What is retained

- `config/templates/` — SOUL/USER/TOOLS presets (discord, telegram, ops, custom)
- `control-plane-ui/` — reference static UI for development
- `CONTRIBUTING.md` — branch/PR workflow (updated for Rust)
- Product intent: hosted OpenClaw instance lifecycle orchestration

## What is removed

- All JS source files in `openclaw-hosting-blueprint-lib/`
- `package.json`
- `InMemoryJobRunner`, `MockSandboxRuntimeAdapter`, `HostedInstanceService`
- Node.js HTTP server (`createHttpService.js`)
- `CODEX_TASK.md`, `CODEX_OPENCLAW_TASK.md`

# Migration: JS Scaffold → Rust Blueprint

## What changed

The original JS-centric scaffold has been replaced with a blueprint-sdk-native
Rust implementation. The product intent (OpenClaw instance orchestration)
is preserved, but the implementation path is now Rust.

## Mapping

| JS scaffold | Rust blueprint |
|---|---|
| `package.json` + `npm start` | `Cargo.toml` workspace + `cargo run` |
| `openclaw-sandbox-blueprint-lib/src/jobs/lifecycleJobs.js` | `openclaw-sandbox-blueprint-lib/src/jobs/lifecycle.rs` |
| `openclaw-sandbox-blueprint-lib/src/services/instanceService.js` | `openclaw-sandbox-blueprint-lib/src/state.rs` |
| `openclaw-sandbox-blueprint-lib/src/runtime/mockSandboxRuntimeAdapter.js` | `openclaw-sandbox-blueprint-lib/src/runtime_adapter.rs` (`InstanceRuntimeAdapter` + `LocalStateRuntimeAdapter`) |
| `openclaw-sandbox-blueprint-lib/src/server/createHttpService.js` | `openclaw-sandbox-blueprint-lib/src/operator_api.rs` + runner wiring in `openclaw-sandbox-blueprint-bin/src/main.rs` |
| `openclaw-sandbox-blueprint-lib/src/server/bootstrap.js` | `openclaw-sandbox-blueprint-bin/src/main.rs` |
| `openclaw-sandbox-blueprint-lib/src/config/templateLoader.js` | Template loading (planned, packs retained in config/) |
| `openclaw-sandbox-blueprint-lib/bin/dev-server.js` | `cargo run --bin openclaw-sandbox-blueprint` |
| `openclaw-sandbox-blueprint-lib/bin/smoke-test.js` | `cargo test --all` |
| HTTP POST `/jobs/create-instance` | On-chain job ID 0 (`create_instance`) |
| HTTP POST `/jobs/start-instance` | On-chain job ID 1 (`start_instance`) |
| HTTP POST `/jobs/stop-instance` | On-chain job ID 2 (`stop_instance`) |
| HTTP POST `/jobs/delete-instance` | On-chain job ID 3 (`delete_instance`) |
| HTTP GET `/instances` | Operator HTTP API query (implemented) |
| HTTP GET `/instances/:id` | Operator HTTP API query (implemented) |
| HTTP GET `/templates` | Operator HTTP API query (implemented) |

## What is retained

- `config/templates/` — SOUL/USER/TOOLS presets (discord, telegram, ops, custom)
- `control-plane-ui/` — reference static UI for development
- `CONTRIBUTING.md` — branch/PR workflow (updated for Rust)
- Product intent: OpenClaw instance lifecycle orchestration

## What is removed

- All JS source files in `openclaw-sandbox-blueprint-lib/`
- `package.json`
- `InMemoryJobRunner`, `MockSandboxRuntimeAdapter`, `InstanceService`
- Node.js HTTP server (`createHttpService.js`)
- `CODEX_TASK.md`, `CODEX_OPENCLAW_TASK.md`

# OpenClaw Instance Blueprint Spec

Blueprint-SDK-native Rust implementation for OpenClaw instance
orchestration on the Tangle network.

## Implementation scope

- **Rust + blueprint-sdk architecture.** The primary implementation path is Rust
  using `blueprint-sdk` crate patterns (sol! types, TangleLayer, BlueprintRunner).
- **Lifecycle state changes are on-chain jobs only:**
  `create` (0), `start` (1), `stop` (2), `delete` (3).
- **Execution target metadata** is persisted per instance:
  `standard` (default) or `tee`.
- **TEE runner variant** is provided as a dedicated binary:
  `openclaw-tee-sandbox-blueprint`.
- **Read-only operations are query surfaces only:**
  instance list, instance detail, template list, health check.
- **Template packs** define SOUL/USER/TOOLS presets for Discord, Telegram, Ops,
  and Custom mode.
- **File-backed persistent state** for instance records with ownership tracking.
- **Caller ownership validation** on all lifecycle operations.
- **Runtime backend abstraction** with real Docker lifecycle execution support:
  create/start/stop/delete invoke Docker when `OPENCLAW_RUNTIME_BACKEND=docker`.

## Out of scope

- Real Firecracker/microVM orchestration (delegated to sandbox runtime).
- Multi-tenant hardened operator API deployment (rate limiting, WAF, external IdP).
- Durable queueing beyond file-backed JSON store.
- Billing and production policy enforcement.
- Control-plane UI backend (static reference UI retained for development).

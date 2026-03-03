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
  `openclaw-tee-instance-blueprint`.
- **Read-only operations are query surfaces only:**
  instance list, instance detail, template list, health check.
- **Template packs** define SOUL/USER/TOOLS presets for Discord, Telegram, Ops,
  and Custom mode.
- **File-backed persistent state** for instance records with ownership tracking.
- **Caller ownership validation** on all lifecycle operations.

## Out of scope

- Real Firecracker/VM orchestration (delegated to sandbox runtime).
- Operator HTTP API for queries (planned, not yet wired).
- Durable queueing beyond file-backed JSON store.
- AuthN/AuthZ, billing, and production policy enforcement.
- Control-plane UI backend (static reference UI retained for development).

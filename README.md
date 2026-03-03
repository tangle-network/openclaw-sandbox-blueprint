![Tangle Network Banner](https://raw.githubusercontent.com/tangle-network/tangle/refs/heads/main/assets/Tangle%20%20Banner.png)

# openclaw-sandbox-blueprint

[![Discord](https://img.shields.io/badge/Discord-Join%20Chat-7289da?logo=discord&logoColor=white)](https://discord.gg/cv8EfJu3Tn)
[![Twitter](https://img.shields.io/twitter/follow/tangle_network?style=social)](https://twitter.com/tangle_network)

Blueprint-SDK-native Rust implementation for orchestrating hosted OpenClaw
instances on the Tangle network.

## Architecture

This is a **blueprint-sdk blueprint** — a Rust workspace that compiles to a
Tangle-compatible binary. The runner listens for on-chain `JobSubmitted` events,
executes the matching handler, and submits results back to the chain.

### Workspace layout

```
Cargo.toml                         # workspace root
openclaw-hosting-blueprint-lib/    # library: sol! types, jobs, state, router
openclaw-hosting-blueprint-bin/    # binary: runner entry point (main.rs)
config/templates/                  # template packs (SOUL/USER/TOOLS presets)
control-plane-ui/                  # reference control-plane UI (static HTML/JS)
docs/                              # architecture notes
```

### Jobs (state-changing, on-chain)

| ID | Name | Description |
|----|------|-------------|
| 0 | `create` | Provision a new hosted OpenClaw instance |
| 1 | `start` | Start a stopped instance |
| 2 | `stop` | Stop a running instance |
| 3 | `delete` | Mark an instance as deleted |

### Queries (read-only, operator HTTP API)

Read-only operations are **not** on-chain jobs. They are served via the
operator HTTP API (planned):

- Instance list
- Instance detail
- Template list
- Health check

### Template packs

Pre-configured SOUL/USER/TOOLS presets in `config/templates/`:

- `discord` — community support and moderation
- `telegram` — customer outreach and bot workflows
- `ops` — operational runbook and incident response
- `custom` — bring your own configuration

## Quickstart

### Build

```bash
cargo check --all-features
cargo build --release
```

### Test

```bash
cargo test --all
```

### Run (requires Tangle node + service registration)

```bash
SERVICE_ID=<id> HTTP_RPC_ENDPOINT=<url> KEYSTORE_URI=<uri> \
  cargo run --release --bin openclaw-hosting-blueprint
```

## Dependency on sandbox-runtime contracts

This blueprint is a **product layer** over sandbox runtime contracts. It does
not implement VM/Firecracker orchestration directly. The runtime adapter
boundary is defined in the lib crate and will be wired to real sandbox-runtime
contract calls as the runtime becomes available.

## Engineering workflow

- State-changing operations are jobs only (`create`, `start`, `stop`, `delete`).
- Read-only operations stay in query surfaces (operator HTTP API).
- Ship in small, composable PRs with explicit validation evidence.

See [CONTRIBUTING.md](CONTRIBUTING.md) for branch, commit, and PR standards.
See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for detailed architecture notes.

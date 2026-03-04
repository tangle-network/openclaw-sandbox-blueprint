![Tangle Network Banner](https://raw.githubusercontent.com/tangle-network/tangle/refs/heads/main/assets/Tangle%20%20Banner.png)

# openclaw-sandbox-blueprint

[![Discord](https://img.shields.io/badge/Discord-Join%20Chat-7289da?logo=discord&logoColor=white)](https://discord.gg/cv8EfJu3Tn)
[![Twitter](https://img.shields.io/twitter/follow/tangle_network?style=social)](https://twitter.com/tangle_network)

Blueprint-SDK-native Rust implementation for orchestrating OpenClaw
instances on the Tangle network.

## Architecture

This is a **blueprint-sdk blueprint** — a Rust workspace that compiles to a
Tangle-compatible binary. The runner listens for on-chain `JobSubmitted` events,
executes the matching handler, and submits results back to the chain.

### Workspace layout

```
Cargo.toml                         # workspace root
openclaw-instance-blueprint-lib/    # library: sol! types, jobs, state, router
openclaw-instance-blueprint-bin/    # binary: runner entry point (main.rs)
openclaw-tee-instance-blueprint-lib/ # TEE variant library wrapper
openclaw-tee-instance-blueprint-bin/ # TEE variant runner binary
config/templates/                  # template packs (SOUL/USER/TOOLS presets)
control-plane-ui/                  # reference control-plane UI (static HTML/JS)
docs/                              # architecture notes
```

### Jobs (state-changing, on-chain)

| ID | Name | Description |
|----|------|-------------|
| 0 | `create` | Provision a new OpenClaw instance |
| 1 | `start` | Start a stopped instance |
| 2 | `stop` | Stop a running instance |
| 3 | `delete` | Mark an instance as deleted |

### Out-of-box variants

Instances support these built-in `claw_variant` values in `config_json`:

- `openclaw`
- `nanoclaw`
- `ironclaw`

Each instance record persists `claw_variant` plus `ui_access` metadata so
operators can route to the correct runtime profile without changing job ABI.

See [docs/VARIANT-REFERENCE.md](docs/VARIANT-REFERENCE.md) for verified source
references and naming-collision notes.

### Queries (read-only, operator HTTP API)

Read-only operations are **not** on-chain jobs. They are served via the
operator HTTP API:

- `GET /` (serves the built-in control-plane UI shell)
- `GET /health`
- `GET /templates`
- `GET /instances` (requires bearer auth)
- `GET /instances/{id}` (requires bearer auth)
- `GET /instances/{id}/access` (requires scoped bearer auth; returns per-instance UI bearer token)
- `POST /instances/{id}/setup/start` (requires scoped bearer auth)
- `POST /instances/{id}/ssh` (requires scoped bearer auth; provision SSH public key)
- `DELETE /instances/{id}/ssh` (requires scoped bearer auth; revoke SSH public key)
- `POST /instances/{id}/terminals` (requires scoped bearer auth; create terminal session)
- `GET /instances/{id}/terminals/{terminalId}/stream` (SSE terminal stream; bearer via header or `?token=`)
- `POST /instances/{id}/terminals/{terminalId}/execute` (requires scoped bearer auth)
- `DELETE /instances/{id}/terminals/{terminalId}` (requires scoped bearer auth)
- `GET /instances/{id}/session/sessions` (requires scoped bearer auth; list chat sessions)
- `POST /instances/{id}/session/sessions` (requires scoped bearer auth; create chat session)
- `PATCH /instances/{id}/session/sessions/{sessionId}` (requires scoped bearer auth; rename session)
- `DELETE /instances/{id}/session/sessions/{sessionId}` (requires scoped bearer auth)
- `GET /instances/{id}/session/sessions/{sessionId}/messages` (requires scoped bearer auth)
- `POST /instances/{id}/session/sessions/{sessionId}/messages` (requires scoped bearer auth)
- `POST /instances/{id}/session/sessions/{sessionId}/abort` (requires scoped bearer auth)
- `GET /instances/{id}/session/events` (SSE chat stream; query `sessionId`, bearer via header or `?token=`)

Session endpoints:

- `POST /auth/challenge` (wallet-signature challenge)
- `POST /auth/session/wallet` (challenge verify -> bearer token)
- `POST /auth/session/token` (access-token auth -> bearer token)

Operator API runtime defaults:

- disabled unless `OPENCLAW_OPERATOR_HTTP_ENABLED=true` (or `1`)
- bind address defaults to `127.0.0.1:8787` via `OPENCLAW_OPERATOR_HTTP_ADDR`

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
./scripts/ci/run-docker-integration-tests.sh
./scripts/ci/run-real-variant-runtime-tests.sh # real OpenClaw + IronClaw + NanoClaw (upstream build)
```

### Run (requires Tangle node + service registration)

```bash
SERVICE_ID=<id> HTTP_RPC_ENDPOINT=<url> KEYSTORE_URI=<uri> \
  cargo run --release --bin openclaw-instance-blueprint
```

### Create config schema (variant + secure UI tunnel)

`create_instance` keeps ABI compatibility (`name`, `template_pack_id`,
`config_json`) and reads optional profile settings from `config_json`:

```json
{
  "claw_variant": "openclaw",
  "ui": {
    "expose_public_url": true,
    "subdomain": "team-assistant",
    "auth_mode": "wallet_signature"
  }
}
```

Supported values:

- `claw_variant`: `openclaw` | `nanoclaw` | `ironclaw`
- `ui.auth_mode`: `wallet_signature` | `access_token`
- `ui.expose_public_url`: defaults to `true`
- when `ui.auth_mode=access_token`, session login uses operator-side
  `OPENCLAW_UI_ACCESS_TOKEN` (not `config_json`)

Tunnel/public URL behavior:

- If `OPENCLAW_UI_BASE_DOMAIN` is set, URL is generated as
  `https://<subdomain>.<OPENCLAW_UI_BASE_DOMAIN>` and tunnel status is `active`.
- If unset, tunnel status is `pending` and URL remains unset until ingress is
  provisioned by runtime infrastructure.
- `owner_only` defaults to `true` for secure-by-default routing.

Execution target behavior:

- Default execution target is `standard`.
- Set `OPENCLAW_EXECUTION_TARGET=tee` to mark new instances as TEE-targeted.
- The dedicated `openclaw-tee-instance-blueprint` binary sets this automatically.

## Dependency on sandbox-runtime contracts

This blueprint is a **product layer** over sandbox runtime contracts. It does
not implement VM/Firecracker orchestration directly. It does implement a real
Docker execution backend for lifecycle operations. The runtime adapter boundary
is defined and wired in the lib crate:

- `InstanceRuntimeAdapter` trait = product/runtime integration contract
- `LocalStateRuntimeAdapter` = default local projection adapter
- `DockerRuntimeAdapter` = lifecycle execution through Docker CLI (`create/start/stop/rm`)

Job handlers call the adapter, not storage internals directly. A future
sandbox-runtime-backed adapter (for microVM/Firecracker) can be injected
without rewriting job handlers.

## Real execution backend (Docker)

Enable Docker-backed lifecycle execution:

```bash
export OPENCLAW_RUNTIME_BACKEND=docker
export OPENCLAW_IMAGE_OPENCLAW=ghcr.io/<org>/<openclaw-image>:<tag>
export OPENCLAW_IMAGE_IRONCLAW=ghcr.io/<org>/<ironclaw-image>:<tag>
export OPENCLAW_DOCKER_PULL=true # optional, default true
```

NanoClaw image options:

- prebuilt image:
  - `OPENCLAW_IMAGE_NANOCLAW=<image:tag>`
- or build on demand during adapter init:
  - `OPENCLAW_NANOCLAW_BUILD_CONTEXT=/path/to/nanoclaw`
  - optional `OPENCLAW_NANOCLAW_BUILD_SCRIPT=container/build.sh`
  - optional `OPENCLAW_NANOCLAW_BUILD_IMAGE_NAME=nanoclaw-agent`
  - optional `OPENCLAW_NANOCLAW_BUILD_TAG=latest`

Behavior:

- `create` creates a container from the variant-mapped image.
- UI port mapping defaults:
  - OpenClaw: `18789`
  - IronClaw: `18789`
  - NanoClaw: inferred from image metadata or explicit env override
- per-variant UI port override: `OPENCLAW_VARIANT_<OPENCLAW|NANOCLAW|IRONCLAW>_UI_PORT`
- per-variant startup command override (runs as `sh -lc <command>`):
  - `OPENCLAW_VARIANT_<OPENCLAW|NANOCLAW|IRONCLAW>_CONTAINER_COMMAND`
- per-variant shell entrypoint override (when image has restrictive `ENTRYPOINT`):
  - `OPENCLAW_VARIANT_<OPENCLAW|NANOCLAW|IRONCLAW>_FORCE_SHELL_ENTRYPOINT`
- per-variant host env passthrough allowlist (comma-separated):
  - `OPENCLAW_VARIANT_<OPENCLAW|NANOCLAW|IRONCLAW>_CONTAINER_ENV_KEYS`
  - values are read from the runner host env and injected as container env
- startup stabilization check:
  - `OPENCLAW_DOCKER_STARTUP_STABILIZE_MS` (default `1000`)
  - if container exits immediately after `start`, lifecycle now fails fast with recent logs
- `start` runs `docker start`.
- `stop` runs `docker stop`.
- `delete` runs `docker rm -f`.
- query surfaces include runtime metadata (`backend`, image, container status, local UI URL, setup status, last error).
- canonical UI auth env is unified across variants: `SANDBOX_UI_BEARER_TOKEN` (`SANDBOX_UI_AUTH_MODE=bearer`).
- per-instance token retrieval for owner-scoped sessions: `GET /instances/{id}/access`.
- canonical env naming + token generation come from
  `sandbox-runtime::ingress_access_control` (re-exported by `openclaw-instance-blueprint-lib`).
- compatibility aliases are still injected for existing images (`CLAW_UI_BEARER_TOKEN`, `OPENCLAW_GATEWAY_TOKEN`, `NANOCLAW_UI_BEARER_TOKEN`, `GATEWAY_AUTH_TOKEN`).
- setup bootstrap can be triggered with `POST /instances/{id}/setup/start` (scoped session required).
- default setup commands:
  - OpenClaw: `openclaw onboard`
  - NanoClaw: no default setup command (owner drives setup via terminal/chat flow)
  - IronClaw: `ironclaw onboard`
- command override per variant: `OPENCLAW_VARIANT_<...>_SETUP_COMMAND`
- setup env allowlist per variant: `OPENCLAW_VARIANT_<...>_SETUP_ENV_KEYS` (comma-separated)
- optional chat command per variant:
  - `OPENCLAW_VARIANT_<OPENCLAW|NANOCLAW|IRONCLAW>_CHAT_COMMAND`
  - command runs inside the container and receives prompt through env var `OPENCLAW_CHAT_PROMPT`

This repository does not publish or bundle the variant images. You must provide
valid image references for your environment.

Real-image runtime notes:

- official OpenClaw image (`ghcr.io/openclaw/openclaw`) is loopback-bound by default.
  The runtime applies a host-reachable startup command automatically for this image.
- official IronClaw worker image requires non-interactive auth env to avoid startup prompts.
  Provide `NEARAI_API_KEY` or `NEARAI_SESSION_TOKEN` in the runner host env.
- NanoClaw upstream `container/build.sh` image is an agent-runner image. The runtime
  now applies a secure hosted bridge command profile for `nanoclaw-agent:*` by
  default (token-gated minimal UI on port `18789`) so instance provisioning and
  owner-scoped setup surfaces stay reachable. You can still override with
  `OPENCLAW_VARIANT_NANOCLAW_CONTAINER_COMMAND`.

Agent UI compatibility:

- `@tangle-network/agent-ui` terminal/session hooks are compatible when using
  `apiUrl=http://<operator>/instances/<instance-id>` with scoped session bearer
  token.

## Security posture (current)

- Containers are bound to loopback only (`127.0.0.1` port mapping), not exposed directly on public interfaces.
- Each Docker instance receives a unique bearer token under canonical env key `SANDBOX_UI_BEARER_TOKEN`.
- NanoClaw hosted bridge profile enforces the per-instance bearer token at HTTP surface level.
- Operator API setup execution is restricted to **instance-scoped sessions** (owner flow), not operator-wide tokens.
- UI token retrieval is restricted to **instance-scoped sessions** (owner flow), not operator-wide tokens.
- Setup env keys are validated and only injected for the setup execution call; they are not persisted in instance state.
- UI ingress should still be fronted by authenticated tunnel/reverse proxy before internet exposure.

## State location

Instance state persists at:

- `$OPENCLAW_INSTANCE_STATE_DIR/instances.json` (preferred)
- fallback: `$OPENCLAW_STATE_DIR/instances.json` (compatibility path)
- default: `/tmp/openclaw-instance-blueprint/instances.json`

## TEE variant

Run the dedicated TEE variant binary:

```bash
SERVICE_ID=<id> HTTP_RPC_ENDPOINT=<url> KEYSTORE_URI=<uri> \
  cargo run --release --bin openclaw-tee-instance-blueprint
```

## Engineering workflow

- State-changing operations are jobs only (`create`, `start`, `stop`, `delete`).
- Read-only operations stay in query surfaces (operator HTTP API).
- Ship in small, composable PRs with explicit validation evidence.
- CI runs Rust checks plus synthetic Docker integration tests in
  `.github/workflows/ci.yml`.
- Use `scripts/ci/run-real-variant-runtime-tests.sh` before releases to verify
  official OpenClaw/IronClaw runtime images plus NanoClaw upstream build path.

See [CONTRIBUTING.md](CONTRIBUTING.md) for branch, commit, and PR standards.
See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for detailed architecture notes.

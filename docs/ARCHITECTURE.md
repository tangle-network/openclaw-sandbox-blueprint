# OpenClaw Instance Blueprint — Architecture

## Overview

This repository is a **blueprint-sdk blueprint** for orchestrating OpenClaw
instances on the Tangle network. It follows the standard blueprint-sdk
patterns: Rust workspace, `sol!`-defined ABI types, `TangleLayer`-wrapped job
handlers, and a `BlueprintRunner` entry point.

## Product-layer scope

This repository is the product-layer blueprint. It is **not** the
infrastructure substrate. Runtime isolation, VM orchestration, and low-level
network/security enforcement are delegated to the sandbox runtime (see
`ai-agent-sandbox-blueprint` for the runtime reference).
Within product-layer scope, this repo can execute lifecycle through a Docker
backend; Firecracker/microVM substrate remains delegated.

## Crate structure

### `openclaw-instance-blueprint-lib`

Library crate containing all business logic:

- **`lib.rs`** — ABI type definitions (`sol!` macro), job ID constants, and
  the `Router` that maps job IDs to handlers via `TangleLayer`.
- **`jobs/lifecycle.rs`** — Handler implementations for `create`, `start`,
  `stop`, and `delete` operations. Each handler receives `Caller`, `CallId`,
  and `TangleArg<T>` extractors and returns `TangleResult<T>`. Handlers call
  the runtime adapter boundary instead of touching storage directly.
- **`runtime_adapter.rs`** — Runtime adapter contract (`InstanceRuntimeAdapter`)
  and implementations (`LocalStateRuntimeAdapter`, `DockerRuntimeAdapter`).
- **`query.rs`** — reusable read-only query helpers (instance/template views).
- **`auth.rs`** — challenge/session auth service for operator API access control.
- **`operator_api.rs`** — axum router and handlers for `/health`,
  `/templates`, `/instances`, auth/session endpoints, and setup trigger endpoint.
- **`state.rs`** — File-backed persistent store for `InstanceRecord` objects.
  Uses `once_cell::OnceCell` + `Mutex<BTreeMap>` with JSON persistence.
- **`error.rs`** — Domain error type (`InstanceError`) with conversions to
  `String` for on-chain error reporting.

### `openclaw-instance-blueprint-bin`

Binary crate with the runner entry point:

- **`main.rs`** — Loads `BlueprintEnvironment`, connects to Tangle, creates
  `TangleProducer`/`TangleConsumer`, and starts `BlueprintRunner` with the
  library's `router()`.

### `openclaw-tee-instance-blueprint-lib` / `openclaw-tee-instance-blueprint-bin`

TEE variant wrappers over the instance blueprint:

- `openclaw-tee-instance-blueprint-lib` re-exports the base library and exposes
  `tee_router()` which forces `OPENCLAW_EXECUTION_TARGET=tee`.
- `openclaw-tee-instance-blueprint-bin` runs the shared lifecycle router with
  TEE execution target preconfigured.

## Jobs vs queries

### Jobs (on-chain, state-changing)

All state mutations go through on-chain jobs. Each job:

1. Is triggered by a `JobSubmitted` event on-chain.
2. Receives ABI-encoded input via `TangleArg`.
3. Validates caller ownership.
4. Persists the state change.
5. Returns ABI-encoded output via `TangleResult`.

| ID | Handler | Transition |
|----|---------|------------|
| 0 | `create_instance` | — → `Stopped` |
| 1 | `start_instance` | `Stopped` → `Running` |
| 2 | `stop_instance` | `Running` → `Stopped` |
| 3 | `delete_instance` | `Stopped`/`Running` → `Deleted` |

### Queries (off-chain, read-only)

Read-only operations are **not** jobs. They are served via the operator
HTTP API (axum):

- `GET /instances` — list instances (scoped by bearer claims)
- `GET /instances/{id}` — instance detail
- `GET /instances/{id}/access` — fetch per-instance UI bearer token (scoped session only)
- `POST /instances/{id}/setup/start` — trigger variant setup bootstrap (scoped session only)
- `GET /templates` — list template packs
- `GET /health` — liveness check

Auth/session endpoints:

- `POST /auth/challenge` — create wallet challenge
- `POST /auth/session/wallet` — verify wallet signature and issue session
- `POST /auth/session/token` — access-token login and session issuance

Operator API startup defaults:

- disabled by default (`OPENCLAW_OPERATOR_HTTP_ENABLED=true` to enable)
- default bind when enabled: `127.0.0.1:8787`

## State management

Instance records are stored in a JSON file at:

- `$OPENCLAW_INSTANCE_STATE_DIR/instances.json` (preferred)
- fallback: `$OPENCLAW_STATE_DIR/instances.json` (legacy compatibility)
- default: `/tmp/openclaw-instance-blueprint/instances.json`

The store uses `once_cell::OnceCell` for lazy initialization and
`std::sync::Mutex` for thread safety. All writes persist to disk immediately.

## Template packs

Template packs live in `config/templates/` and define SOUL/USER/TOOLS presets
for different use cases. Each pack contains:

- `template.json` — metadata (id, name, mode, description)
- `SOUL.md` — agent identity and guardrails
- `USER.md` — target audience and goals
- `TOOLS.md` — tool access matrix

## Adapter boundary

The adapter boundary is implemented:

- `InstanceRuntimeAdapter` is the lifecycle contract consumed by product jobs.
- `LocalStateRuntimeAdapter` is the default adapter (file-backed local state).
- `DockerRuntimeAdapter` executes real container lifecycle via Docker CLI when
  `OPENCLAW_RUNTIME_BACKEND=docker` and image env vars are configured.
- Canonical UI auth env key across variants is `CLAW_UI_BEARER_TOKEN`
  (`CLAW_UI_AUTH_MODE=bearer`), with variant-specific aliases set for compatibility.
- Runtime maps UI ports to loopback host addresses only (`127.0.0.1`), so
  public exposure must happen through an authenticated tunnel/reverse proxy layer.
- `init_instance_runtime_adapter(...)` allows replacing the default with a
  sandbox-runtime-backed adapter.

This keeps lifecycle handlers stable while runtime backend implementations
evolve.

## Variant and UI ingress model

Create requests accept optional `config_json` profile settings:

- `claw_variant`: `openclaw` | `nanoclaw` | `ironclaw`
- `ui.expose_public_url`: bool (default `true`)
- `ui.subdomain`: optional preferred subdomain
- `ui.auth_mode`: `wallet_signature` | `access_token`
- `access_token` sessions validate against `OPENCLAW_UI_ACCESS_TOKEN` (operator-side env)

Persisted instance records include:

- `claw_variant` (typed enum)
- `execution_target` (`standard` | `tee`)
- `ui_access.public_url`
- `ui_access.tunnel_status` (`pending` | `active` | `disabled`)
- `ui_access.auth_mode`
- `ui_access.owner_only` (default `true`)

When `OPENCLAW_UI_BASE_DOMAIN` is set, lifecycle create computes
`https://<subdomain>.<base_domain>` as the instance public URL.

Canonical variant-source references live in `docs/VARIANT-REFERENCE.md`.

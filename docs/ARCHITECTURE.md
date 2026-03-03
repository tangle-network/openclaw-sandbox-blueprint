# OpenClaw Hosting Blueprint — Architecture

## Overview

This repository is a **blueprint-sdk blueprint** for orchestrating hosted
OpenClaw instances on the Tangle network. It follows the standard blueprint-sdk
patterns: Rust workspace, `sol!`-defined ABI types, `TangleLayer`-wrapped job
handlers, and a `BlueprintRunner` entry point.

## Product-layer scope

This repository is the product-layer blueprint. It is **not** the
infrastructure substrate. Runtime isolation, VM orchestration, and low-level
network/security enforcement are delegated to the sandbox runtime (see
`ai-agent-sandbox-blueprint` for the runtime reference).

## Crate structure

### `openclaw-hosting-blueprint-lib`

Library crate containing all business logic:

- **`lib.rs`** — ABI type definitions (`sol!` macro), job ID constants, and
  the `Router` that maps job IDs to handlers via `TangleLayer`.
- **`jobs/lifecycle.rs`** — Handler implementations for `create`, `start`,
  `stop`, and `delete` operations. Each handler receives `Caller`, `CallId`,
  and `TangleArg<T>` extractors and returns `TangleResult<T>`.
- **`state.rs`** — File-backed persistent store for `InstanceRecord` objects.
  Uses `once_cell::OnceCell` + `Mutex<BTreeMap>` with JSON persistence.
- **`error.rs`** — Domain error type (`HostingError`) with conversions to
  `String` for on-chain error reporting.

### `openclaw-hosting-blueprint-bin`

Binary crate with the runner entry point:

- **`main.rs`** — Loads `BlueprintEnvironment`, connects to Tangle, creates
  `TangleProducer`/`TangleConsumer`, and starts `BlueprintRunner` with the
  library's `router()`.

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

Read-only operations are **not** jobs. They will be served via the operator
HTTP API (axum) in a future iteration:

- `GET /instances` — list all instances
- `GET /instances/:id` — instance detail
- `GET /templates` — list template packs
- `GET /health` — liveness check

## State management

Instance records are stored in a JSON file at
`$OPENCLAW_STATE_DIR/instances.json` (default: `/tmp/openclaw-hosting-blueprint/instances.json`).

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

The lib crate defines the lifecycle operations (create, start, stop, delete)
with a clear boundary where real sandbox-runtime contract calls will be wired.
Currently the handlers manage state directly; when the runtime becomes
available, an adapter trait will be introduced to delegate to the runtime API.

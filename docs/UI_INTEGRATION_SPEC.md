# OpenClaw UI Integration Spec

## 1) Purpose

Define a clean, reusable UI architecture for OpenClaw provisioning and operations using:

- `@tangle-network/blueprint-ui` for chain + provisioning concerns
- `@tangle-network/agent-ui` for terminal/chat/session experience

This spec is grounded in current repository state and avoids duplicate app-local implementations that already exist in shared packages.

## 2) Current State (Audit)

### 2.1 OpenClaw UI status

- `ui/` is now a React control-plane app using:
  - `@tangle-network/blueprint-ui` primitives/shell components
  - `@tangle-network/agent-ui` terminal + chat/session surfaces
- `control-plane-ui/` now stores build artifacts generated from `ui/` and served by operator API.
- Lifecycle actions (`create/start/stop/delete`) are on-chain only and currently not available in the control-plane UI.

### 2.2 Product and API surfaces already implemented

OpenClaw already exposes:

- On-chain lifecycle jobs via router in `openclaw-instance-blueprint-lib`:
  - `0=create`, `1=start`, `2=stop`, `3=delete`
- Operator API for read/query/auth/setup/terminal/chat/tee/ssh flows.

### 2.3 Shared packages status

- `@tangle-network/blueprint-ui` already provides service/operator/quote/job hooks and reusable form/shell components.
- `@tangle-network/agent-ui` already provides terminal and chat/session hooks/components.
- Important compatibility note:
  - `agent-ui` terminal/session hooks are compatible with OpenClaw instance-scoped paths.
  - `agent-ui` generic auth hook currently targets `/api/auth/*`, while OpenClaw auth is `/auth/*`.

## 3) Architecture Decision

Use one React app (`ui/`) with two domains:

1. Provisioning domain (on-chain): blueprint-ui-first
2. Runtime operations domain (off-chain owner session): agent-ui-first

Keep `control-plane-ui/` as generated artifact output consumed by the Rust operator API.

## 4) Package Boundaries

### 4.1 Put in `@tangle-network/blueprint-ui`

- Wallet/network shell integration
- Blueprint/service/operator discovery and validation
- RFQ quote collection and service creation UX
- Generic on-chain job form execution patterns
- Shared infra modal/shell patterns that are app-agnostic

### 4.2 Put in `@tangle-network/agent-ui`

- Session chat rendering and stream handling
- PTY terminal lifecycle and rendering
- Generic session CRUD hooks
- Agent-specific message/run/tool UX

### 4.3 Keep app-local in OpenClaw `ui/`

- OpenClaw blueprint definitions and field transforms
- OpenClaw auth adapter for `/auth/challenge|/auth/session/*`
- OpenClaw product-specific provisioning defaults/copy
- TEE-specific UX for sealed secrets and attestation

## 5) Provisioning UX Spec

## 5.1 Primary route map

- `/create` -> provision new OpenClaw instance (on-chain create job)
- `/instances` -> list instances + status
- `/instances/:id` -> detail with lifecycle actions + terminal/chat/setup/auth + tee tools

## 5.2 Provisioning flow (`/create`)

1. Select infrastructure
   - Choose blueprint ID and service ID (existing service) or create new service from operators/quotes.
2. Validate permissions
   - Validate service active/permitted for connected wallet.
3. Configure create job
   - Input name/template/variant/auth/subdomain/public-url settings.
4. Submit on-chain `create` job
   - Encode ABI args and submit via wallet.
5. Confirm transaction and present next action
   - Navigate to instance detail on success.

## 5.3 Default vs advanced controls

Default-visible fields:

- `name`
- `templatePackId`
- `clawVariant`
- `uiAuthMode`

Advanced section:

- `ui.expose_public_url`
- `ui.subdomain`
- raw `config_json` override (expert mode only)

Rule: build `config_json` from structured defaults; allow raw JSON override only when explicit toggle is enabled.

## 5.4 Lifecycle actions in instance detail

On detail page:

- Show `start` when state is `stopped`
- Show `stop` when state is `running`
- Show `delete` when state is not `deleted`

Actions submit on-chain jobs using same shared job-execution pattern.

## 6) Integration Contract with `blueprint-ui`

## 6.1 Required hooks/components

- `useServiceValidation`
- `useOperators`
- `useQuotes`
- `useJobForm`
- `useSubmitJob`
- `BlueprintJobForm`
- `JobExecutionDialog`
- `infraStore`/`updateInfra`

## 6.2 OpenClaw blueprint definition in app

Define OpenClaw blueprint in `ui/src/lib/blueprints/openclaw-blueprint.ts`:

- Job 0 `create`
- Job 1 `start`
- Job 2 `stop`
- Job 3 `delete`

`create` form fields map to ABI:

- `name` -> `name`
- `templatePackId` -> `template_pack_id`
- `configJson` -> `config_json`

Before submit, transform structured UI fields into `configJson`:

```json
{
  "claw_variant": "openclaw|nanoclaw|ironclaw",
  "ui": {
    "expose_public_url": true,
    "subdomain": "optional",
    "auth_mode": "wallet_signature|access_token"
  }
}
```

## 6.3 Service creation behavior

Support both:

- `createServiceFromQuotes` when quote set exists
- `requestService` fallback when quotes unavailable

Persist selected service and operators in `infraStore` for subsequent job submission.

## 7) Integration Contract with `agent-ui`

## 7.1 Base URL contract

For instance-scoped operations, use:

- `instanceApiBase = <operator-api-origin>/instances/<instanceId>`

Then:

- terminal hooks call `${instanceApiBase}/terminals...`
- session hooks call `${instanceApiBase}/session/...`

## 7.2 Auth model for OpenClaw

Implement app-local `useOpenClawSessionAuth` (do not use `useSidecarAuth` directly yet):

- `POST /auth/challenge` with `{ instanceId, walletAddress }`
- sign returned message in wallet
- `POST /auth/session/wallet` with `{ challengeId, signature }`
- or `POST /auth/session/token` with `{ instanceId, accessToken }`
- cache returned bearer token per `{instanceId, operatorApiOrigin}`

After token acquisition:

- pass token to `useSessions`, `useCreateSession`, `useSessionStream`, `TerminalView`

## 7.3 Chat + terminal composition

- Chat panel: `ChatContainer` + `useSessionStream`
- Terminal panel: `TerminalView`
- Session list/create/rename/delete: `useSessionCrud` hooks

## 8) TEE UX Spec

For instances with `execution_target=tee`:

Enable in detail route:

- `GET /tee/public-key`
- `POST /tee/sealed-secrets`
- `GET /tee/attestation`

Hide or disable these controls for non-TEE instances.

## 9) Reusability and Dedup Plan

1. Build OpenClaw app using shared primitives first.
2. Extract any reused infra selection modal into `blueprint-ui` if used by 2+ apps.
3. Keep OpenClaw-specific auth adapter app-local until generalized endpoint strategy is added to `agent-ui`.
4. Keep `control-plane-ui` as generated build output from `ui/` (no hand-maintained JS UI code).

## 10) Performance and Reliability Requirements

- Avoid repeated operator log scans by caching operator discovery per blueprint in-memory for session lifetime.
- Debounce quote refresh requests and allow manual refresh.
- Keep chat/terminal streams isolated so reconnecting one does not reset the other.
- Use optimistic disabled states for lifecycle buttons to prevent duplicate submits.
- Route-level code-split heavy terminal components (`React.lazy` for terminal view).

## 11) Testing Requirements

## 11.1 Unit tests (UI)

- `config_json` transform correctness from form fields.
- Lifecycle button visibility gating by instance state.
- Auth adapter request/response mapping for both wallet and access-token login.

## 11.2 Integration tests (UI + mocked network)

- Create flow: infra validation -> submit create job.
- Instance detail: start/stop/delete submissions encode proper ABI args.
- Chat/terminal hooks against mocked instance-scoped endpoints.

## 11.3 E2E smoke (real local stack)

- Create instance on-chain.
- Open detail view.
- Acquire session bearer (wallet or token).
- Open terminal, execute command.
- Send chat message and receive streamed reply.
- If TEE target enabled: fetch public key + attestation and send sealed secret payload.

## 12) Migration Plan

Phase 1 (foundation)

- Replace Vite starter in `ui/` with real app shell + routing + wallet providers.
- Add OpenClaw blueprint definition and create/detail/list routes.

Phase 2 (provisioning)

- Wire infra modal, service validation, operator discovery, quotes, and create/start/stop/delete jobs.

Phase 3 (runtime operations)

- Add OpenClaw auth adapter and agent-ui terminal/chat integration.
- Add TEE tooling panel.

Phase 4 (cleanup)

- Remove hand-maintained static JS UI code.
- Keep `control-plane-ui/` as generated production artifact path for operator embedding.

## 13) Acceptance Criteria

This work is complete when:

1. OpenClaw provisioning is fully functional from React UI using `blueprint-ui` hooks/components.
2. Runtime chat/terminal/session flows run through `agent-ui` against real OpenClaw instance-scoped endpoints.
3. No duplicate local implementations exist for shared infra/job/session primitives already provided by shared packages.
4. `control-plane-ui/` is generated from `ui/` and no duplicated hand-maintained UI logic remains.

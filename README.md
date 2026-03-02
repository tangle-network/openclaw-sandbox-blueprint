# openclaw-sandbox-blueprint

Product-layer scaffold for hosted OpenClaw instances running on top of sandbox runtime contracts.

## Workspace Shape

- `openclaw-hosting-blueprint-lib/bin` - runnable entrypoints (`dev-server`, `smoke-test`)
- `openclaw-hosting-blueprint-lib/src` - product-layer HTTP service, jobs, runtime adapter boundary
- `control-plane-ui` - minimal control-plane page for template selection and launch
- `config/templates` - template packs (`SOUL.md`, `USER.md`, `TOOLS.md`)

## Implemented

- Lifecycle jobs for hosted instance state changes only:
  - `POST /jobs/create-hosted-instance`
  - `POST /jobs/start-hosted-instance`
  - `POST /jobs/stop-hosted-instance`
  - `POST /jobs/delete-hosted-instance`
- Read-only HTTP service endpoints (not jobs):
  - `GET /instances`
  - `GET /instances/:id`
  - `GET /templates`
- Template packs for:
  - `discord`
  - `telegram`
  - `ops`
  - `custom` mode
- Minimal control-plane UI that selects a template pack and submits a launch request.
- Local dev `MockSandboxRuntimeAdapter` implementing sandbox-runtime lifecycle contract methods.

## Dependency on sandbox-runtime contracts

This blueprint assumes the sandbox runtime provides the lifecycle interface used by the service:

- `createHostedInstance`
- `startHostedInstance`
- `stopHostedInstance`
- `deleteHostedInstance`

The repository intentionally does not implement VM/firecracker orchestration.

## Quickstart

1. Start the local service and UI:

```bash
npm start
```

2. Open `http://localhost:8787`.

3. Validate the happy path non-interactively:

```bash
npm run smoke
```

## Roadmap

- Replace in-memory job runner with durable queue + retries.
- Replace mock runtime adapter with real sandbox-runtime API client.
- Add authn/authz and multi-tenant project scoping.
- Add audit events and usage/billing hooks.
- Add richer control-plane flows (secrets, network policy, logs, metrics).

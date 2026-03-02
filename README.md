![Tangle Network Banner](https://raw.githubusercontent.com/tangle-network/tangle/refs/heads/main/assets/Tangle%20%20Banner.png)

# openclaw-sandbox-blueprint

[![Discord](https://img.shields.io/badge/Discord-Join%20Chat-7289da?logo=discord&logoColor=white)](https://discord.gg/cv8EfJu3Tn)
[![Twitter](https://img.shields.io/twitter/follow/tangle_network?style=social)](https://twitter.com/tangle_network)

Product-layer scaffold for hosted OpenClaw instances running on top of sandbox runtime contracts.

## Workspace shape

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

## Incremental engineering workflow

- Keep state changes in lifecycle jobs only.
- Keep read-only operations out of jobs and inside query endpoints.
- Ship in small, composable PRs with explicit validation evidence.
- Keep architecture notes current as behavior evolves.

See [CONTRIBUTING.md](CONTRIBUTING.md) for branch, commit, and PR standards.

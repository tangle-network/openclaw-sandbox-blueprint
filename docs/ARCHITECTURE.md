# OpenClaw Hosting Blueprint Architecture

## Product-Layer Scope
This repository is the product-layer blueprint for hosted OpenClaw instances. It is not the infrastructure substrate. Runtime isolation, VM orchestration, and low-level network/security enforcement are delegated to the sandbox runtime.

## Dependency on Sandbox Runtime Contracts
`openclaw-hosting-blueprint-lib` depends on sandbox-runtime contracts for state-changing lifecycle operations:

- `createHostedInstance`
- `startHostedInstance`
- `stopHostedInstance`
- `deleteHostedInstance`

The local development adapter (`MockSandboxRuntimeAdapter`) only simulates those calls.

## Template Packs
Template packs live in `config/templates` and contain SOUL/USER/TOOLS presets. Included packs:

- `discord`
- `telegram`
- `ops`
- `custom` (full custom mode)

## Control Plane + Jobs Boundary
- State-changing actions are jobs only (`POST /jobs/{create|start|stop|delete}-hosted-instance`).
- Read-only operations are HTTP service endpoints (`GET /instances`, `GET /instances/:id`, `GET /templates`).

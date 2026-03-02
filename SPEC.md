# OpenClaw Hosting Blueprint Spec (Scaffold)

This repository provides the initial product-layer blueprint for creating and operating hosted OpenClaw instances on top of sandbox runtime contracts.

Current scaffold scope:

- Lifecycle state changes are job-driven (`create`, `start`, `stop`, `delete`).
- Read-only status/list operations are direct HTTP service endpoints.
- Template packs define SOUL/USER/TOOLS presets for Discord, Telegram, Ops, and Custom mode.
- Control-plane UI supports selecting a template pack and launching an instance.

Out of scope in this scaffold:

- Real Firecracker orchestration.
- Durable storage and queueing.
- AuthN/AuthZ, billing, and production policy enforcement.

# OpenClaw Control Plane UI

React control-plane source app for `openclaw-sandbox-blueprint`.

## Purpose

- Uses shared `@tangle-network/blueprint-ui` primitives for shell + cards + tabs
- Uses shared `@tangle-network/agent-ui` for terminal/chat runtime surfaces
- Builds static artifacts consumed by Rust operator API (`/`, `/app.js`, `/styles.css`, `/assets/*`)

## Commands

```bash
pnpm install
pnpm run dev
pnpm run build
pnpm run build:embedded
```

`build:embedded` compiles `ui/dist` and copies artifacts into `../control-plane-ui/`.

## UX contract

Default path is one-click:

1. Save bearer token
2. Pick instance
3. Start setup
4. Use terminal/chat

Low-level controls (wallet/access-token auth variants, SSH, TEE payload tools, env overrides)
are available under **Advanced** sections.

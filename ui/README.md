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

Primary IA is tabbed and progressive:

1. `Provision` tab: 3-step wizard (`Session` -> `Instance Profile` -> `Review + Launch`)
2. `Instances` tab: choose the active instance
3. `Workspace` tab: setup, terminal, chat, advanced runtime tools
4. `Access` tab: wallet/access-token session flows

Default path stays one-click inside Workspace (`Start Setup`), while low-level controls
(SSH, TEE payload tools, env overrides) remain under **Advanced**.

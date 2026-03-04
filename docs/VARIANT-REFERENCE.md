# Variant Reference (Verified)

Date verified: March 4, 2026

The variant names in this blueprint are intentionally constrained to:

- `openclaw`
- `nanoclaw`
- `ironclaw`

These names are ambiguous on the public internet. Use this reference as the
disambiguation baseline for product/runtime mappings.

## Canonical references used by this repo

### `openclaw`

- https://openclaw.ai/
- https://docs.openclaw.ai/
- https://github.com/openclaw/openclaw
- https://github.com/openclaw/openclaw/pkgs/container/openclaw

### `nanoclaw`

- https://nanoclaw.dev/
- https://github.com/qwibitai/nanoclaw

### `ironclaw`

- https://www.ironclaw.com/
- https://github.com/nearai/ironclaw

## Container runtime status snapshot (March 4, 2026)

- `openclaw`
  - confirmed public image: `ghcr.io/openclaw/openclaw:latest`
  - default image startup binds loopback and is not host-reachable via Docker port publish
  - this blueprint applies a container command profile for official OpenClaw images
    so hosted instance URLs become reachable
- `nanoclaw`
  - no official hosted-service image verified from upstream
  - upstream `container/build.sh` image (`nanoclaw-agent:*`) is a stdin-driven
    agent-runner image and exits immediately without JSON input
  - this blueprint defaults `nanoclaw-agent:*` to a long-lived terminal-first
    runtime profile (`tail -f /dev/null` with shell entrypoint override)
  - canonical setup path is owner-scoped terminal/chat (`claude` then `/setup`)
  - operators can still override runtime command profile using
    `OPENCLAW_VARIANT_NANOCLAW_CONTAINER_COMMAND`
- `ironclaw`
  - confirmed public worker image: `nearaidev/ironclaw-nearai-worker:latest`
  - image hosts web gateway successfully when `NEARAI_API_KEY` or
    `NEARAI_SESSION_TOKEN` is present (non-interactive startup)
  - without auth env, startup blocks on interactive provider auth prompt

## Naming collision warning

All three names have unrelated projects/products with similar names. Runtime
and billing integrations should always key by internal profile IDs, not by
display names alone.

# Variant Reference (Verified)

Date verified: March 3, 2026

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

## Container image status snapshot (March 3, 2026)

- `openclaw`: confirmed public image (`ghcr.io/openclaw/openclaw:latest`)
- `nanoclaw`: project/docs are real, but official public image is unverified;
  repository provides Dockerfile/build scripts for self-building image artifacts
- this blueprint supports NanoClaw script-based image builds via
  `OPENCLAW_NANOCLAW_BUILD_CONTEXT` + `OPENCLAW_NANOCLAW_BUILD_SCRIPT`
- `ironclaw`: public image exists (`nearaidev/ironclaw-nearai-worker:latest`),
  but deployment references are split across registries and should be validated
  per environment before production rollout

## Naming collision warning

All three names have unrelated projects/products with similar names. Runtime
and billing integrations should always key by internal profile IDs, not by
display names alone.

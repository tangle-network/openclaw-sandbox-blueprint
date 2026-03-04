# CLAUDE.md

## Scope

This repo is an instance-blueprint product layer over shared sandbox runtime
contracts. Keep it greenfield-clean: no legacy compatibility shims unless
explicitly required by a live production dependency.

TEE policy for this repo:
- Use `sandbox-runtime` as the single TEE runtime/sealed-secrets primitive layer.
- Do not introduce parallel TEE lifecycle/secrets abstractions in this repo.

## Verified Runtime Truths (March 4, 2026)

- `openclaw` official image: `ghcr.io/openclaw/openclaw:latest`
  - default startup is loopback-bound and not host-reachable through Docker
    published ports.
  - hosted instance flow requires command profile override to bind `lan` and
    satisfy Control UI origin policy.
- `ironclaw` official worker image: `nearaidev/ironclaw-nearai-worker:latest`
  - becomes web-UI reachable when non-interactive auth env is present.
  - without `NEARAI_API_KEY` or `NEARAI_SESSION_TOKEN`, startup can block in
    interactive auth prompts.
- `nanoclaw` upstream `container/build.sh` image (`nanoclaw-agent:*`)
  - is a stdin-driven one-shot runner, not a hosted long-running service image.
  - runtime now uses a terminal-first long-lived profile for hosted instances
    (no native NanoClaw web UI assumption).

## Verified UI Build Truths (March 4, 2026)

- Source UI lives in `ui/` (React + shared `blueprint-ui`/`agent-ui`).
- Operator-served assets live in `control-plane-ui/` and are generated output,
  not hand-edited source.
- Always run `cd ui && pnpm run build:embedded` after UI changes.
- Operator API must serve:
  - `/` + `/app.js` + `/styles.css`
  - `/assets/*` for split chunks.
- Do not force single-file bundles just to avoid serving `/assets/*`; that
  regresses first-load performance.

## Remote Access Contract (March 4, 2026)

- Treat loopback (`127.0.0.1` / `localhost`) as internal-only runtime plumbing.
- User-facing URLs (`publicUrl`, `setupUrl`, UI links shown in cards) must not
  be loopback when remote mode is enabled.
- Keep internal and external URLs distinct:
  - internal: operator-local container reachability
  - external: browser-reachable tunnel/public origin
- Browser RPC must not silently fall back to localhost. Use explicit
  `VITE_RPC_URL` / `VITE_WS_RPC_URL`, or derive from browser host with explicit
  port mapping.
- Operator API client must support explicit base URL config
  (`VITE_OPERATOR_API_BASE_URL`) and build URLs with `new URL(path, base)`.
- Remote mode (`OPENCLAW_REMOTE_ACCESS=1`) must fail fast when outward-facing
  URLs resolve to loopback.

## UI Quality Gates (Required for UI PRs)

- Notification channel must be visible and accessible:
  - render async/system notices in UI
  - include `aria-live` status/alert semantics
- Destructive actions require confirmation/undo paths:
  - instance delete must require explicit confirm
  - session delete must support explicit confirm or undo window
- No hardcoded low-contrast accent text classes in product JSX
  (`text-rose-200|300`, `text-teal-200`, `text-amber-200` on light surfaces).
  Use semantic theme tokens only.
- Form labels must bind to controls via `htmlFor` + `id`.
- Motion must respect `prefers-reduced-motion`.
- Embedded asset parity must be enforced after `pnpm run build:embedded`
  (source `ui/` and generated `control-plane-ui/` remain in sync).

## Do

- Validate behavior against real images, not only placeholder images.
- Fail fast on runtime prerequisites (for example, missing IronClaw auth env).
- Keep default UX one-click and move low-level controls into explicit
  "Advanced" sections.
- Keep synthetic CI and real-image CI separate:
  - synthetic lane for fast deterministic checks
  - real-image lane for weekly/manual production-adjacent proof
- Record evidence with exact commands and outcomes in PR descriptions.
- For UI changes, record both:
  - `pnpm run build:embedded`
  - `cargo test -p openclaw-sandbox-blueprint-lib`

## Do Not

- Do not claim a variant is hosted-runtime ready unless a real image path has
  passed create/start/reachability checks.
- Do not rely on default container entrypoints for official images without
  verifying network reachability and auth startup behavior.
- Do not run one-shot variant entrypoints directly in hosted mode without an
  explicit hosted command profile.
- Do not ship hand-maintained UI logic in `control-plane-ui/`; it must be
  generated from `ui/`.
- Do not expose loopback setup/public URLs to remote users.
- Do not ship browser RPC/API defaults that require end-user localhost to work.

## Verified Flows

- Fast synthetic Docker lane:
  - `./scripts/ci/run-docker-integration-tests.sh`
- Real-image runtime lane (OpenClaw + IronClaw + NanoClaw upstream build path):
  - `./scripts/ci/run-real-variant-runtime-tests.sh`
- Full workspace tests:
  - `cargo test --workspace`

Expected real-image pass signal:

- `docker_real_variant_runtime_matrix ... ok`

## Failure Triage

- If real-image lane fails with `timed out waiting for HTTP UI`:
  - list lingering variant containers:
    - `docker ps -a --filter label=openclaw.instance_id`
  - clear stale containers:
    - `docker rm -f $(docker ps -aq --filter label=openclaw.instance_id)`
  - rerun real-image lane.
- If OpenClaw is unreachable, re-check Control UI origin requirements and
  startup command profile.
- If IronClaw is unreachable, re-check `NEARAI_*` env and startup logs.

## Current CI Contract

- `ci.yml` runs fmt/lint/unit + synthetic Docker integration.
- `real-variant-runtime.yml` runs weekly/manual real-image runtime validation.

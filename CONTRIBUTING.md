# Contributing

## Branching

- Never push directly to `main`.
- Use short-lived branches:
  - `feat/<scope>`
  - `fix/<scope>`
  - `chore/<scope>`
  - `docs/<scope>`

## Commit quality

- Follow Conventional Commit style.
- Keep each commit focused on one concern.
- Exclude local task notes (for example `CODEX_*`) from commits.
- Do not include co-author trailers unless explicitly requested.

## Local qualification

Automatic PR/main-push CI is retired. Merge the current default branch and qualify the affected source through the maintained admitted gate. The comprehensive Rust reference retains the previous formatting, Clippy, workspace-test and advisory checks:

```bash
beelink-gate auto https://github.com/tangle-network/openclaw-sandbox-blueprint.git <full-sha> -- bash -lc 'cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --workspace && cargo audit'
```

The admitted host needs Rust with rustfmt/Clippy, protoc and cargo-audit. Cargo manifests own SDK revisions and vendored patches. Compilation through Clippy precedes the workspace tests. Workflow/docs-only changes use YAML, content/link and exact-diff checks.

The retained `scripts/ci/run-docker-integration-tests.sh` exercises actual Docker with synthetic images. Run it separately on an admitted disposable hostlab Docker host; its label-based cleanup must not share a production or another lane's Docker daemon. The distinct real-image `scripts/ci/run-real-variant-runtime-tests.sh` remains required for release/runtime claims, on the same disposable ownership basis with its explicit authentication prerequisites. Source checks and synthetic images do not establish hosted variant readiness. Neither this source qualification nor a passing local script authorizes on-chain state changes, production deployment or model requests.

UI changes retain the existing embedded build/parity and library qualification in [CLAUDE.md](CLAUDE.md#verified-ui-build-truths-march-4-2026).

## PR standards

Each PR should include:

- Problem and intent
- Scope and non-goals
- Validation evidence (`cargo test`, `cargo check`, `cargo clippy`)
- Risks and follow-up items

## Architecture guardrails

- This is a **blueprint-sdk blueprint** (Rust). The JS scaffold has been replaced.
- Lifecycle mutations are on-chain jobs only (`create`, `start`, `stop`, `delete`).
- Read-only operations stay in query surfaces (operator HTTP API), not jobs.
- This repository remains a product layer over sandbox-runtime contracts.
- Job IDs must be sequential and match the on-chain contract.

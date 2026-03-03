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

## Building and testing

```bash
cargo check --all-features
cargo test --all
cargo clippy --all-features -- -D warnings
```

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

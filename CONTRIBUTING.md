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

## PR standards

Each PR should include:

- Problem and intent
- Scope and non-goals
- Validation evidence (`npm run smoke`, `/health` checks, unit/integration tests)
- Risks and follow-up items

## Architecture guardrails

- Lifecycle mutations are jobs only (`create/start/stop/delete`).
- Read-only operations stay query-only (`/instances`, `/instances/:id`, `/templates`).
- This repository remains a product layer over sandbox-runtime contracts.

#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

if ! command -v docker >/dev/null 2>&1; then
  echo "docker CLI is required"
  exit 1
fi

if ! docker info >/dev/null 2>&1; then
  echo "docker daemon is not reachable"
  exit 1
fi

cleanup() {
  local ids
  ids="$(docker ps -aq --filter "label=openclaw.instance_id" || true)"
  if [[ -n "${ids}" ]]; then
    docker rm -f ${ids} >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

retry_pull() {
  local image="$1"
  local attempts=5
  local delay=2
  local i=1
  while [[ "$i" -le "$attempts" ]]; do
    if docker pull "$image" >/dev/null 2>&1; then
      return 0
    fi
    echo "pull failed for $image (attempt $i/$attempts)"
    sleep "$delay"
    i=$((i + 1))
  done
  echo "failed to pull $image after $attempts attempts"
  exit 1
}

retry_pull "nginx:alpine"
retry_pull "alpine:3.20"

# Keep this lane fast/deterministic with synthetic docker images.
cargo test -p openclaw-sandbox-blueprint-lib runtime_adapter::tests::docker_ -- --ignored --test-threads=1
cargo test -p openclaw-sandbox-blueprint-lib docker_operator_api_control_plane_e2e -- --ignored --test-threads=1

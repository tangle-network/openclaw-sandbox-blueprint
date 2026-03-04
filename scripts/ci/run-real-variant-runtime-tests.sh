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

# Defaults are pinned to upstream canonical images; callers can override.
: "${OPENCLAW_IMAGE_OPENCLAW:=ghcr.io/openclaw/openclaw:latest}"
: "${OPENCLAW_IMAGE_IRONCLAW:=nearaidev/ironclaw-nearai-worker:latest}"
: "${OPENCLAW_REAL_INCLUDE_NANOCLAW:=true}"
: "${OPENCLAW_REAL_UI_TIMEOUT_SECS:=150}"

if [[ "${OPENCLAW_REAL_INCLUDE_NANOCLAW}" == "0" || "${OPENCLAW_REAL_INCLUDE_NANOCLAW,,}" == "false" ]]; then
  export OPENCLAW_REAL_INCLUDE_NANOCLAW=false
else
  export OPENCLAW_REAL_INCLUDE_NANOCLAW=true
fi

if [[ -z "${NEARAI_API_KEY:-}" && -z "${NEARAI_SESSION_TOKEN:-}" ]]; then
  echo "NEARAI_API_KEY/NEARAI_SESSION_TOKEN not set; using placeholder key for non-interactive IronClaw startup"
  export NEARAI_API_KEY="integration-placeholder-key"
fi

echo "pulling ${OPENCLAW_IMAGE_OPENCLAW}"
docker pull "${OPENCLAW_IMAGE_OPENCLAW}"

echo "pulling ${OPENCLAW_IMAGE_IRONCLAW}"
docker pull "${OPENCLAW_IMAGE_IRONCLAW}"

if [[ "${OPENCLAW_REAL_INCLUDE_NANOCLAW}" == "true" && -z "${OPENCLAW_IMAGE_NANOCLAW:-}" ]]; then
  if [[ -z "${OPENCLAW_NANOCLAW_BUILD_CONTEXT:-}" ]]; then
    if ! command -v git >/dev/null 2>&1; then
      echo "git CLI is required to build NanoClaw from upstream source context"
      exit 1
    fi
    OPENCLAW_NANOCLAW_BUILD_CONTEXT="$(mktemp -d /tmp/openclaw-nanoclaw-src-XXXXXX)"
    echo "cloning NanoClaw upstream into ${OPENCLAW_NANOCLAW_BUILD_CONTEXT}"
    git clone --depth=1 https://github.com/qwibitai/nanoclaw "${OPENCLAW_NANOCLAW_BUILD_CONTEXT}"
  fi
  echo "building NanoClaw image from context ${OPENCLAW_NANOCLAW_BUILD_CONTEXT}"
  (
    cd "${OPENCLAW_NANOCLAW_BUILD_CONTEXT}"
    CONTAINER_RUNTIME=docker ./container/build.sh "${OPENCLAW_NANOCLAW_BUILD_TAG:-latest}"
  )
  export OPENCLAW_IMAGE_NANOCLAW="${OPENCLAW_NANOCLAW_BUILD_IMAGE_NAME:-nanoclaw-agent}:${OPENCLAW_NANOCLAW_BUILD_TAG:-latest}"
fi

export OPENCLAW_RUNTIME_BACKEND=docker
export OPENCLAW_AUTO_TRIGGER_SETUP=false
export OPENCLAW_DOCKER_STARTUP_STABILIZE_MS=1500
export OPENCLAW_REAL_UI_TIMEOUT_SECS

run_matrix() {
  cargo test -p openclaw-instance-blueprint-lib docker_real_variant_runtime_matrix -- --ignored --test-threads=1
}

if ! run_matrix; then
  echo "real variant runtime matrix failed once; retrying with extended UI timeout"
  export OPENCLAW_REAL_UI_TIMEOUT_SECS="$((OPENCLAW_REAL_UI_TIMEOUT_SECS + 60))"
  run_matrix
fi

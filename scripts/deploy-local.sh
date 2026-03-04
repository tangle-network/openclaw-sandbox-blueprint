#!/usr/bin/env bash
# deploy-local.sh — Full local Tangle lifecycle deploy for OpenClaw instance blueprints.
#
# This script follows the same blueprint lifecycle pattern as other production repos:
#   1) Start local Anvil with Tangle state snapshot
#   2) Register OpenClaw instance + TEE blueprints on-chain
#   3) Register operators for both blueprints
#   4) Request + approve services on-chain
#   5) Build binaries/UI
#   6) Start instance + TEE operators against real service IDs
#   7) (Optional) submit on-chain create job smoke test
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPTS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

ANVIL_PORT="${ANVIL_PORT:-8745}"
ANVIL_CHAIN_ID="${ANVIL_CHAIN_ID:-31338}"
RPC_URL="${RPC_URL:-http://127.0.0.1:$ANVIL_PORT}"
WS_RPC_URL="${WS_RPC_URL:-ws://127.0.0.1:$ANVIL_PORT}"
OPERATOR_API_PORT="${OPERATOR_API_PORT:-8787}"
TEE_OPERATOR_API_PORT="${TEE_OPERATOR_API_PORT:-8788}"
BUILD_UI="${BUILD_UI:-1}"
SKIP_BUILD="${SKIP_BUILD:-0}"
RUN_SMOKE_CREATE="${RUN_SMOKE_CREATE:-0}"
OPENCLAW_RUNTIME_BACKEND="${OPENCLAW_RUNTIME_BACKEND:-docker}"
OPENCLAW_IMAGE_OPENCLAW="${OPENCLAW_IMAGE_OPENCLAW:-ghcr.io/openclaw/openclaw:latest}"
OPENCLAW_IMAGE_IRONCLAW="${OPENCLAW_IMAGE_IRONCLAW:-nearaidev/ironclaw-nearai-worker:latest}"
OPENCLAW_NANOCLAW_AUTO_BOOTSTRAP="${OPENCLAW_NANOCLAW_AUTO_BOOTSTRAP:-1}"
OPENCLAW_NANOCLAW_BUILD_CONTEXT="${OPENCLAW_NANOCLAW_BUILD_CONTEXT:-}"
OPENCLAW_NANOCLAW_CACHE_DIR="${OPENCLAW_NANOCLAW_CACHE_DIR:-/tmp/openclaw-nanoclaw-upstream}"
OPENCLAW_NANOCLAW_REPO_URL="${OPENCLAW_NANOCLAW_REPO_URL:-https://github.com/qwibitai/nanoclaw.git}"

# Prefer Tailscale host when available so browser wallets can reach RPC remotely.
if [[ -z "${PUBLIC_HOST:-}" ]]; then
    mapfile -t TS_IPS < <(tailscale ip -4 2>/dev/null || true)
    if [[ ${#TS_IPS[@]} -gt 1 ]]; then
        echo "WARNING: multiple Tailscale IPv4 addresses detected (${TS_IPS[*]})."
        echo "         Using first address ${TS_IPS[0]}. Set PUBLIC_HOST explicitly to override."
    fi
    PUBLIC_HOST="${TS_IPS[0]:-127.0.0.1}"
fi

# UI/browser-facing RPC URLs (used by wagmi and job submissions in the web app).
PUBLIC_HTTP_RPC_URL="${PUBLIC_HTTP_RPC_URL:-$RPC_URL}"
PUBLIC_WS_RPC_URL="${PUBLIC_WS_RPC_URL:-$WS_RPC_URL}"
PUBLIC_HTTP_RPC_URL="${PUBLIC_HTTP_RPC_URL//127.0.0.1/$PUBLIC_HOST}"
PUBLIC_HTTP_RPC_URL="${PUBLIC_HTTP_RPC_URL//localhost/$PUBLIC_HOST}"
PUBLIC_WS_RPC_URL="${PUBLIC_WS_RPC_URL//127.0.0.1/$PUBLIC_HOST}"
PUBLIC_WS_RPC_URL="${PUBLIC_WS_RPC_URL//localhost/$PUBLIC_HOST}"

ANVIL_STATE="${ANVIL_STATE:-$(cd "$ROOT_DIR/.." && pwd)/blueprint/crates/chain-setup/anvil/snapshots/localtestnet-state.json}"

# Deterministic Anvil accounts
DEPLOYER_KEY="0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
DEPLOYER_ADDR="0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
OPERATOR1_KEY="0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d"
OPERATOR1_ADDR="0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
OPERATOR2_KEY="0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a"
OPERATOR2_ADDR="0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC"
USER_KEY="0x8b3a350cf5c34c9194ca85829a2df0ec3153be0318b5e2d3348e872092edffba"
USER_ADDR="0x9965507D1a55bcC2695C58ba16FB37d819B0A4dc"
EXTRA_FUNDED_ADDRS="${EXTRA_FUNDED_ADDRS:-0xd04E36A1C370c6115e1C676838AcD0b430d740F3}"
EXTRA_PROVISION_CALLERS="${EXTRA_PROVISION_CALLERS:-$EXTRA_FUNDED_ADDRS}"
FUND_TNT_WEI="${FUND_TNT_WEI:-100000000000000000000}" # 100 TNT/native units on local chain

# Tangle local snapshot addresses
TANGLE="0xCf7Ed3AccA5a467e9e704C703E8D87F634fB0Fc9"
RESTAKING="0xe7f1725E7734CE288F8367e1Bb143E90bb3F0512"
STATUS_REGISTRY="0xdC64a140Aa3E981100a9BecA4E685f962f0CF6C9"

OPENCLAW_OPERATOR_API_TOKEN="${OPENCLAW_OPERATOR_API_TOKEN:-oclw_dev_operator_token}"
OPENCLAW_UI_ACCESS_TOKEN="${OPENCLAW_UI_ACCESS_TOKEN:-oclw_dev_access_token}"
OPENCLAW_ALLOW_WALLET_SIGNATURE_ACCESS_TOKEN_FALLBACK="${OPENCLAW_ALLOW_WALLET_SIGNATURE_ACCESS_TOKEN_FALLBACK:-1}"

cleanup() {
    if [[ "${CLEANED_UP:-0}" == "1" ]]; then
        return
    fi
    CLEANED_UP=1
    echo ""
    echo "Shutting down local stack..."
    if [[ "${ANVIL_STARTED:-0}" == "1" ]]; then
        [[ -n "${ANVIL_PID:-}" ]] && kill "$ANVIL_PID" 2>/dev/null || true
    fi
    [[ -n "${INSTANCE_OPERATOR_PID:-}" ]] && kill "$INSTANCE_OPERATOR_PID" 2>/dev/null || true
    [[ -n "${TEE_OPERATOR_PID:-}" ]] && kill "$TEE_OPERATOR_PID" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

require_cmd() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "ERROR: required command not found: $1"
        exit 1
    fi
}

for cmd in anvil cast forge cargo curl; do
    require_cmd "$cmd"
done

if [[ "$OPENCLAW_RUNTIME_BACKEND" == "docker" ]]; then
    require_cmd docker
    require_cmd git

    if [[ -z "${OPENCLAW_IMAGE_NANOCLAW:-}" && -z "${OPENCLAW_NANOCLAW_BUILD_CONTEXT:-}" ]]; then
        if [[ "$OPENCLAW_NANOCLAW_AUTO_BOOTSTRAP" == "1" || "$OPENCLAW_NANOCLAW_AUTO_BOOTSTRAP" == "true" ]]; then
            echo "  Bootstrapping NanoClaw build context into $OPENCLAW_NANOCLAW_CACHE_DIR"
            if [[ -d "$OPENCLAW_NANOCLAW_CACHE_DIR/.git" ]]; then
                git -C "$OPENCLAW_NANOCLAW_CACHE_DIR" fetch --depth 1 origin main >/dev/null 2>&1 || true
                git -C "$OPENCLAW_NANOCLAW_CACHE_DIR" reset --hard FETCH_HEAD >/dev/null 2>&1 || true
            else
                rm -rf "$OPENCLAW_NANOCLAW_CACHE_DIR"
                git clone --depth 1 "$OPENCLAW_NANOCLAW_REPO_URL" "$OPENCLAW_NANOCLAW_CACHE_DIR" >/dev/null
            fi
            OPENCLAW_NANOCLAW_BUILD_CONTEXT="$OPENCLAW_NANOCLAW_CACHE_DIR"
        else
            echo "ERROR: set OPENCLAW_IMAGE_NANOCLAW or OPENCLAW_NANOCLAW_BUILD_CONTEXT when OPENCLAW_RUNTIME_BACKEND=docker"
            exit 1
        fi
    fi

    if [[ ! -d "${OPENCLAW_NANOCLAW_BUILD_CONTEXT:-/__missing__}" && -z "${OPENCLAW_IMAGE_NANOCLAW:-}" ]]; then
        echo "ERROR: OPENCLAW_NANOCLAW_BUILD_CONTEXT does not exist: $OPENCLAW_NANOCLAW_BUILD_CONTEXT"
        exit 1
    fi

    # The upstream IronClaw worker image requires non-interactive auth env.
    if [[ -z "${NEARAI_API_KEY:-}" && -z "${NEARAI_SESSION_TOKEN:-}" ]]; then
        export NEARAI_API_KEY="${NEARAI_API_KEY:-integration-placeholder-key}"
        echo "  WARNING: NEARAI_API_KEY/NEARAI_SESSION_TOKEN not set; using placeholder key for local startup."
    fi
fi

export OPENCLAW_IMAGE_OPENCLAW
export OPENCLAW_IMAGE_IRONCLAW
export OPENCLAW_IMAGE_NANOCLAW="${OPENCLAW_IMAGE_NANOCLAW:-}"
export OPENCLAW_NANOCLAW_BUILD_CONTEXT="${OPENCLAW_NANOCLAW_BUILD_CONTEXT:-}"
export OPENCLAW_NANOCLAW_BUILD_SCRIPT="${OPENCLAW_NANOCLAW_BUILD_SCRIPT:-container/build.sh}"
export OPENCLAW_NANOCLAW_BUILD_IMAGE_NAME="${OPENCLAW_NANOCLAW_BUILD_IMAGE_NAME:-nanoclaw-agent}"
export OPENCLAW_NANOCLAW_BUILD_TAG="${OPENCLAW_NANOCLAW_BUILD_TAG:-latest}"

parse_deploy() {
    echo "$FORGE_OUTPUT" | grep "DEPLOY_${1}=" | sed "s/.*DEPLOY_${1}=//" | tr -d ' '
}

to_dec_u64() {
    local raw="$1"
    raw="$(echo "$raw" | xargs)"
    if [[ "$raw" == 0x* || "$raw" == 0X* ]]; then
        local hex="${raw#0x}"
        hex="${hex#0X}"
        printf '%d\n' "$((16#$hex))"
    else
        echo "$raw"
    fi
}

build_address_array() {
    local csv="$1"
    local -a values=()
    local -A seen=()
    local item normalized

    while IFS= read -r item; do
        normalized="$(echo "$item" | xargs)"
        [[ -z "$normalized" ]] && continue
        if [[ -z "${seen[$normalized]:-}" ]]; then
            seen[$normalized]=1
            values+=("$normalized")
        fi
    done < <(echo "$csv" | tr ', ' '\n' | sed '/^$/d')

    local out="["
    local idx
    for idx in "${!values[@]}"; do
        if [[ "$idx" -gt 0 ]]; then
            out+=","
        fi
        out+="${values[$idx]}"
    done
    out+="]"
    echo "$out"
}

SERVICE_CALLERS_ARRAY="$(build_address_array "$USER_ADDR,$DEPLOYER_ADDR,$EXTRA_PROVISION_CALLERS")"

echo "=== OpenClaw Blueprint — Full Local Deployment ==="
echo "RPC:         $RPC_URL"
echo "Chain ID:    $ANVIL_CHAIN_ID"
echo "Public host: $PUBLIC_HOST"
echo "Runtime:     $OPENCLAW_RUNTIME_BACKEND"
echo "Callers:     $SERVICE_CALLERS_ARRAY"
if [[ "$OPENCLAW_RUNTIME_BACKEND" == "docker" ]]; then
    echo "OpenClaw:    $OPENCLAW_IMAGE_OPENCLAW"
    echo "IronClaw:    $OPENCLAW_IMAGE_IRONCLAW"
    if [[ -n "${OPENCLAW_IMAGE_NANOCLAW:-}" ]]; then
        echo "NanoClaw:    $OPENCLAW_IMAGE_NANOCLAW"
    else
        echo "NanoClaw:    build from $OPENCLAW_NANOCLAW_BUILD_CONTEXT"
    fi
fi
echo ""

wait_for_http_ready() {
    local url="$1"
    local expected_pattern="$2"
    local deadline=$((SECONDS + 40))
    until curl -s -o /dev/null -w '%{http_code}' "$url" 2>/dev/null | grep -Eq "$expected_pattern"; do
        if [[ $SECONDS -ge $deadline ]]; then
            echo "  WARNING: endpoint not ready in time: $url"
            return
        fi
        sleep 1
    done
}

fund_account_tnt() {
    local addr="$1"
    local amount_wei="$2"
    if [[ -z "$addr" ]]; then
        return
    fi
    local amount_hex
    amount_hex="$(cast to-hex "$amount_wei")"
    cast rpc --rpc-url "$RPC_URL" anvil_setBalance "$addr" "$amount_hex" >/dev/null
    local funded_wei funded_tnt
    funded_wei="$(cast balance "$addr" --rpc-url "$RPC_URL" 2>/dev/null || echo "$amount_wei")"
    funded_tnt="$(cast from-wei "$funded_wei" ether 2>/dev/null || echo "$funded_wei")"
    echo "  Funded $addr with $funded_tnt TNT (native)"
}

# ── [0/11] Start Anvil ───────────────────────────────────────────────────────
echo "[0/11] Starting Anvil..."
ANVIL_STARTED=0
if cast block-number --rpc-url "$RPC_URL" >/dev/null 2>&1; then
    echo "  Reusing existing Anvil at $RPC_URL"
else
    if [[ -f "$ANVIL_STATE" ]]; then
        anvil --block-time 2 --host 0.0.0.0 --port "$ANVIL_PORT" \
            --chain-id "$ANVIL_CHAIN_ID" --disable-code-size-limit --load-state "$ANVIL_STATE" --silent &
        echo "  Loaded snapshot: $ANVIL_STATE"
    else
        echo "  WARNING: snapshot missing at $ANVIL_STATE, starting empty Anvil"
        anvil --block-time 2 --host 0.0.0.0 --port "$ANVIL_PORT" \
            --chain-id "$ANVIL_CHAIN_ID" --disable-code-size-limit --silent &
    fi
    ANVIL_PID=$!
    ANVIL_STARTED=1
    sleep 2
fi
cast block-number --rpc-url "$RPC_URL" >/dev/null
if [[ "$ANVIL_STARTED" == "1" ]]; then
    echo "  Anvil PID: $ANVIL_PID"
fi

echo "  Funding caller wallets (gas + TNT)..."
fund_account_tnt "$USER_ADDR" "$FUND_TNT_WEI"
for EXTRA_ADDR in $(echo "$EXTRA_FUNDED_ADDRS" | tr ', ' '\n' | sed '/^$/d'); do
    fund_account_tnt "$EXTRA_ADDR" "$FUND_TNT_WEI"
done

# ── [1/11] Ensure Foundry deps for RegisterBlueprint script ─────────────────
echo "[1/11] Ensuring Solidity dependencies..."
if [[ ! -d "$ROOT_DIR/dependencies/tnt-core-0.10.1" || ! -d "$ROOT_DIR/dependencies/forge-std-1.9.6" ]]; then
    (cd "$ROOT_DIR" && forge soldeer update -d >/dev/null)
    echo "  Installed dependencies via forge soldeer"
else
    echo "  Dependencies already present"
fi

# ── [2/11] Register blueprints on Tangle ────────────────────────────────────
echo "[2/11] Registering instance + TEE blueprints..."
FORGE_OUTPUT="$(forge script "$ROOT_DIR/contracts/script/RegisterBlueprint.s.sol" \
    --rpc-url "$RPC_URL" --broadcast --slow 2>&1)" || {
    echo "ERROR: RegisterBlueprint failed:"
    echo "$FORGE_OUTPUT" | tail -40
    exit 1
}

INSTANCE_BLUEPRINT_ID="$(parse_deploy INSTANCE_BLUEPRINT_ID)"
TEE_INSTANCE_BLUEPRINT_ID="$(parse_deploy TEE_INSTANCE_BLUEPRINT_ID)"
if [[ -z "$INSTANCE_BLUEPRINT_ID" || -z "$TEE_INSTANCE_BLUEPRINT_ID" ]]; then
    echo "ERROR: failed to parse blueprint IDs from forge output"
    echo "$FORGE_OUTPUT" | tail -40
    exit 1
fi
echo "  Instance blueprint ID:    $INSTANCE_BLUEPRINT_ID"
echo "  TEE instance blueprint ID: $TEE_INSTANCE_BLUEPRINT_ID"

# ── [3/11] Register operators ────────────────────────────────────────────────
echo "[3/11] Registering operators..."
OPERATOR1_PUBKEY_RAW="$(cast wallet public-key --private-key "$OPERATOR1_KEY" 2>/dev/null | head -1)"
OPERATOR2_PUBKEY_RAW="$(cast wallet public-key --private-key "$OPERATOR2_KEY" 2>/dev/null | head -1)"
OPERATOR1_PUBKEY="0x04${OPERATOR1_PUBKEY_RAW#0x}"
OPERATOR2_PUBKEY="0x04${OPERATOR2_PUBKEY_RAW#0x}"

OPERATOR1_RPC="http://$PUBLIC_HOST:$OPERATOR_API_PORT"
OPERATOR2_RPC="http://$PUBLIC_HOST:$TEE_OPERATOR_API_PORT"

for BP_ID in "$INSTANCE_BLUEPRINT_ID" "$TEE_INSTANCE_BLUEPRINT_ID"; do
    if ! cast send "$TANGLE" "registerOperator(uint64,bytes,string)" \
        "$BP_ID" "$OPERATOR1_PUBKEY" "$OPERATOR1_RPC" \
        --gas-limit 2000000 --rpc-url "$RPC_URL" --private-key "$OPERATOR1_KEY" >/dev/null 2>&1; then
        cast send "$TANGLE" "updateOperatorPreferences(uint64,bytes,string)" \
            "$BP_ID" 0x "$OPERATOR1_RPC" \
            --gas-limit 2000000 --rpc-url "$RPC_URL" --private-key "$OPERATOR1_KEY" >/dev/null 2>&1 || true
    fi

    if ! cast send "$TANGLE" "registerOperator(uint64,bytes,string)" \
        "$BP_ID" "$OPERATOR2_PUBKEY" "$OPERATOR2_RPC" \
        --gas-limit 2000000 --rpc-url "$RPC_URL" --private-key "$OPERATOR2_KEY" >/dev/null 2>&1; then
        cast send "$TANGLE" "updateOperatorPreferences(uint64,bytes,string)" \
            "$BP_ID" 0x "$OPERATOR2_RPC" \
            --gas-limit 2000000 --rpc-url "$RPC_URL" --private-key "$OPERATOR2_KEY" >/dev/null 2>&1 || true
    fi
done

for BP_ID in "$INSTANCE_BLUEPRINT_ID" "$TEE_INSTANCE_BLUEPRINT_ID"; do
    OP1_REG="$(cast call "$TANGLE" "isOperatorRegistered(uint64,address)(bool)" "$BP_ID" "$OPERATOR1_ADDR" --rpc-url "$RPC_URL" 2>/dev/null)"
    OP2_REG="$(cast call "$TANGLE" "isOperatorRegistered(uint64,address)(bool)" "$BP_ID" "$OPERATOR2_ADDR" --rpc-url "$RPC_URL" 2>/dev/null)"
    if [[ "$OP1_REG" != "true" || "$OP2_REG" != "true" ]]; then
        echo "ERROR: operator registration incomplete for blueprint #$BP_ID (op1=$OP1_REG op2=$OP2_REG)"
        exit 1
    fi
done
echo "  Operator 1: $OPERATOR1_ADDR -> $OPERATOR1_RPC"
echo "  Operator 2: $OPERATOR2_ADDR -> $OPERATOR2_RPC"

# ── [4/11] Request services ──────────────────────────────────────────────────
echo "[4/11] Requesting services..."
NEXT_REQ="$(cast call "$TANGLE" "serviceRequestCount()(uint64)" --rpc-url "$RPC_URL" | xargs)"
NEXT_REQ="$(to_dec_u64 "$NEXT_REQ")"
SVC_BEFORE="$(cast call "$TANGLE" "serviceCount()(uint64)" --rpc-url "$RPC_URL" | xargs)"
SVC_BEFORE="$(to_dec_u64 "$SVC_BEFORE")"

cast send "$TANGLE" "requestService(uint64,address[],bytes,address[],uint64,address,uint256)" \
    "$INSTANCE_BLUEPRINT_ID" \
    "[$OPERATOR1_ADDR,$OPERATOR2_ADDR]" \
    "0x" \
    "$SERVICE_CALLERS_ARRAY" \
    31536000 \
    "0x0000000000000000000000000000000000000000" \
    0 \
    --gas-limit 3000000 --rpc-url "$RPC_URL" --private-key "$DEPLOYER_KEY" >/dev/null
INSTANCE_REQ_ID="$NEXT_REQ"
NEXT_REQ=$((NEXT_REQ + 1))

cast send "$TANGLE" "requestService(uint64,address[],bytes,address[],uint64,address,uint256)" \
    "$TEE_INSTANCE_BLUEPRINT_ID" \
    "[$OPERATOR1_ADDR,$OPERATOR2_ADDR]" \
    "0x" \
    "$SERVICE_CALLERS_ARRAY" \
    31536000 \
    "0x0000000000000000000000000000000000000000" \
    0 \
    --gas-limit 3000000 --rpc-url "$RPC_URL" --private-key "$DEPLOYER_KEY" >/dev/null
TEE_INSTANCE_REQ_ID="$NEXT_REQ"
echo "  Submitted service requests instance=#$INSTANCE_REQ_ID tee=#$TEE_INSTANCE_REQ_ID"

# ── [5/11] Approve services ──────────────────────────────────────────────────
echo "[5/11] Approving services..."
for REQ_ID in "$INSTANCE_REQ_ID" "$TEE_INSTANCE_REQ_ID"; do
    cast send "$TANGLE" "approveService(uint64,uint8)" "$REQ_ID" 100 \
        --gas-limit 10000000 --rpc-url "$RPC_URL" --private-key "$OPERATOR1_KEY" >/dev/null
    cast send "$TANGLE" "approveService(uint64,uint8)" "$REQ_ID" 100 \
        --gas-limit 10000000 --rpc-url "$RPC_URL" --private-key "$OPERATOR2_KEY" >/dev/null
done
echo "  Both operators approved both requests"

# ── [6/11] Resolve service IDs ───────────────────────────────────────────────
echo "[6/11] Resolving service IDs..."
SVC_AFTER="$(cast call "$TANGLE" "serviceCount()(uint64)" --rpc-url "$RPC_URL" | xargs)"
SVC_AFTER="$(to_dec_u64 "$SVC_AFTER")"

INSTANCE_SERVICE_ID=""
TEE_INSTANCE_SERVICE_ID=""
for SVC_ID in $(seq "$SVC_BEFORE" "$((SVC_AFTER - 1))"); do
    SVC_DATA="$(cast call "$TANGLE" "getService(uint64)" "$SVC_ID" --rpc-url "$RPC_URL" 2>/dev/null)"
    BP_WORD="$(echo "$SVC_DATA" | head -c 66)"
    BP_NUM="$(to_dec_u64 "$BP_WORD")"
    if [[ "$BP_NUM" == "$INSTANCE_BLUEPRINT_ID" ]]; then
        INSTANCE_SERVICE_ID="$SVC_ID"
    elif [[ "$BP_NUM" == "$TEE_INSTANCE_BLUEPRINT_ID" ]]; then
        TEE_INSTANCE_SERVICE_ID="$SVC_ID"
    fi
done

if [[ -z "$INSTANCE_SERVICE_ID" || -z "$TEE_INSTANCE_SERVICE_ID" ]]; then
    echo "ERROR: unable to resolve service IDs from created services"
    echo "  expected instance blueprint=$INSTANCE_BLUEPRINT_ID tee blueprint=$TEE_INSTANCE_BLUEPRINT_ID"
    echo "  inspected range: $SVC_BEFORE..$((SVC_AFTER - 1))"
    exit 1
fi
echo "  Instance service ID: $INSTANCE_SERVICE_ID"
echo "  TEE service ID:      $TEE_INSTANCE_SERVICE_ID"

# ── [7/11] Setup keystores ───────────────────────────────────────────────────
echo "[7/11] Importing operator keys into keystores..."
mkdir -p "$SCRIPTS_DIR/data/operator1/keystore" "$SCRIPTS_DIR/data/operator2/keystore"
CARGO_TANGLE="${CARGO_TANGLE_BIN:-$(command -v cargo-tangle 2>/dev/null || echo "")}"
if [[ -z "$CARGO_TANGLE" && -x "$ROOT_DIR/../blueprint/target/release/cargo-tangle" ]]; then
    CARGO_TANGLE="$ROOT_DIR/../blueprint/target/release/cargo-tangle"
fi

if [[ -n "$CARGO_TANGLE" && -x "$CARGO_TANGLE" ]]; then
    "$CARGO_TANGLE" tangle key import --key-type ecdsa \
        --secret "${OPERATOR1_KEY#0x}" \
        --keystore-path "$SCRIPTS_DIR/data/operator1/keystore" >/dev/null 2>&1 || true
    "$CARGO_TANGLE" tangle key import --key-type ecdsa \
        --secret "${OPERATOR2_KEY#0x}" \
        --keystore-path "$SCRIPTS_DIR/data/operator2/keystore" >/dev/null 2>&1 || true
    echo "  Keystore import complete"
else
    if [[ -z "$(ls -A "$SCRIPTS_DIR/data/operator1/keystore" 2>/dev/null)" || -z "$(ls -A "$SCRIPTS_DIR/data/operator2/keystore" 2>/dev/null)" ]]; then
        echo "ERROR: cargo-tangle not found and keystore directories are empty"
        echo "Build it with: cd ../blueprint && cargo build -p cargo-tangle --release"
        exit 1
    fi
    echo "  WARNING: cargo-tangle missing, using existing keystore files"
fi

# ── [8/11] Build embedded UI + binaries ─────────────────────────────────────
if [[ "$BUILD_UI" == "1" ]]; then
    echo "[8/11] Building embedded control-plane UI..."
    (
        cd "$ROOT_DIR/ui"
        export VITE_DEMO_MODE=0
        export VITE_CHAIN_ID="$ANVIL_CHAIN_ID"
        export VITE_CHAIN_NAME="Tangle Local"
        export VITE_CHAIN_CURRENCY_SYMBOL="ETH"
        export VITE_RPC_URL="$PUBLIC_HTTP_RPC_URL"
        export VITE_WS_RPC_URL="$PUBLIC_WS_RPC_URL"
        export VITE_TANGLE_CONTRACT="$TANGLE"
        export VITE_JOBS_ADDRESS="$TANGLE"
        export VITE_SERVICES_ADDRESS="$TANGLE"
        export VITE_INSTANCE_SERVICE_ID="$INSTANCE_SERVICE_ID"
        export VITE_TEE_INSTANCE_SERVICE_ID="$TEE_INSTANCE_SERVICE_ID"
        export VITE_OPERATOR_API_TOKEN="$OPENCLAW_OPERATOR_API_TOKEN"
        pnpm install
        pnpm run build:embedded >/dev/null
    )
    # include_dir! does not always trigger recompilation when only embedded files
    # change; bump Rust source mtimes so rebuilt binaries always pick up fresh UI.
    touch "$ROOT_DIR/openclaw-instance-blueprint-lib/src/operator_api.rs" \
          "$ROOT_DIR/openclaw-tee-instance-blueprint-lib/src/lib.rs"
    echo "  Embedded UI artifacts refreshed"
else
    echo "[8/11] Skipping embedded UI build (BUILD_UI=0)"
fi

if [[ "$SKIP_BUILD" == "1" ]]; then
    echo "  Skipping Rust build (SKIP_BUILD=1)"
    if [[ "$BUILD_UI" == "1" ]]; then
        echo "  WARNING: UI was rebuilt, but binaries were not rebuilt."
        echo "           Embedded assets update only after recompiling operator binaries."
    fi
else
    echo "  Building OpenClaw runners (includes embedded UI assets)..."
    cargo build --release -p openclaw-instance-blueprint-bin -p openclaw-tee-instance-blueprint-bin >/dev/null
    echo "  Runner binaries built"
fi

# ── [9/11] Start operators ───────────────────────────────────────────────────
echo "[9/11] Starting instance + TEE operators..."
mkdir -p "$SCRIPTS_DIR/data/operator1/state" "$SCRIPTS_DIR/data/operator2/state"
if [[ "${RESET_LOCAL_STATE:-1}" == "1" ]]; then
    rm -f "$SCRIPTS_DIR/data/operator1/state/instances.json" "$SCRIPTS_DIR/data/operator2/state/instances.json"
fi

export PROTOCOL=tangle
export HTTP_RPC_URL="$RPC_URL"
export HTTP_RPC_ENDPOINT="$RPC_URL"
export WS_RPC_URL="$WS_RPC_URL"
export TANGLE_CONTRACT="$TANGLE"
export RESTAKING_CONTRACT="$RESTAKING"
export STATUS_REGISTRY_CONTRACT="$STATUS_REGISTRY"
export OPENCLAW_RUNTIME_BACKEND
export OPENCLAW_OPERATOR_HTTP_ENABLED=true
export OPENCLAW_OPERATOR_API_TOKEN
export OPENCLAW_UI_ACCESS_TOKEN
export OPENCLAW_ALLOW_WALLET_SIGNATURE_ACCESS_TOKEN_FALLBACK
export OPENCLAW_AUTH_CHALLENGE_TTL_SECS="${OPENCLAW_AUTH_CHALLENGE_TTL_SECS:-300}"
export OPENCLAW_AUTH_SESSION_TTL_SECS="${OPENCLAW_AUTH_SESSION_TTL_SECS:-21600}"
export RUST_LOG="${RUST_LOG:-info}"

if [[ ! -x "$ROOT_DIR/target/release/openclaw-instance-blueprint" ]]; then
    echo "ERROR: missing binary $ROOT_DIR/target/release/openclaw-instance-blueprint"
    echo "Set SKIP_BUILD=0 or build it manually."
    exit 1
fi
if [[ ! -x "$ROOT_DIR/target/release/openclaw-tee-instance-blueprint" ]]; then
    echo "ERROR: missing binary $ROOT_DIR/target/release/openclaw-tee-instance-blueprint"
    echo "Set SKIP_BUILD=0 or build it manually."
    exit 1
fi

OPENCLAW_OPERATOR_HTTP_ADDR="0.0.0.0:$OPERATOR_API_PORT" \
BLUEPRINT_ID="$INSTANCE_BLUEPRINT_ID" \
SERVICE_ID="$INSTANCE_SERVICE_ID" \
KEYSTORE_URI="$SCRIPTS_DIR/data/operator1/keystore" \
DATA_DIR="$SCRIPTS_DIR/data/operator1/state" \
OPENCLAW_INSTANCE_STATE_DIR="$SCRIPTS_DIR/data/operator1/state" \
OPENCLAW_STATE_DIR="$SCRIPTS_DIR/data/operator1/state" \
"$ROOT_DIR/target/release/openclaw-instance-blueprint" run --test-mode &
INSTANCE_OPERATOR_PID=$!

OPENCLAW_OPERATOR_HTTP_ADDR="0.0.0.0:$TEE_OPERATOR_API_PORT" \
BLUEPRINT_ID="$TEE_INSTANCE_BLUEPRINT_ID" \
SERVICE_ID="$TEE_INSTANCE_SERVICE_ID" \
KEYSTORE_URI="$SCRIPTS_DIR/data/operator2/keystore" \
DATA_DIR="$SCRIPTS_DIR/data/operator2/state" \
OPENCLAW_INSTANCE_STATE_DIR="$SCRIPTS_DIR/data/operator2/state" \
OPENCLAW_STATE_DIR="$SCRIPTS_DIR/data/operator2/state" \
"$ROOT_DIR/target/release/openclaw-tee-instance-blueprint" run --test-mode &
TEE_OPERATOR_PID=$!

wait_for_http_ready "http://127.0.0.1:$OPERATOR_API_PORT/health" "200"
wait_for_http_ready "http://127.0.0.1:$TEE_OPERATOR_API_PORT/health" "200"
echo "  Instance operator PID: $INSTANCE_OPERATOR_PID"
echo "  TEE operator PID:      $TEE_OPERATOR_PID"

# ── [10/11] Optional on-chain create-job smoke ──────────────────────────────
if [[ "$RUN_SMOKE_CREATE" == "1" ]]; then
    echo "[10/11] Running on-chain create job smoke test..."
    SMOKE_NAME="demo-claw-$(date +%s)"
    SMOKE_INPUT="$(cast abi-encode "f(string,string,string)" "$SMOKE_NAME" "ops" "{\"claw_variant\":\"openclaw\"}")"
    cast send "$TANGLE" "submitJob(uint64,uint8,bytes)" \
        "$INSTANCE_SERVICE_ID" 0 "$SMOKE_INPUT" \
        --gas-limit 3000000 --rpc-url "$RPC_URL" --private-key "$DEPLOYER_KEY" >/dev/null

    FOUND_SMOKE=0
    for _ in $(seq 1 25); do
        BODY="$(curl -s -H "Authorization: Bearer $OPENCLAW_OPERATOR_API_TOKEN" \
            "http://127.0.0.1:$OPERATOR_API_PORT/instances" || true)"
        if echo "$BODY" | grep -q "\"name\":\"$SMOKE_NAME\""; then
            FOUND_SMOKE=1
            break
        fi
        sleep 1
    done

    if [[ "$FOUND_SMOKE" == "1" ]]; then
        echo "  Smoke create job succeeded: $SMOKE_NAME"
    else
        echo "  WARNING: smoke job submitted but instance list did not reflect $SMOKE_NAME yet"
    fi
else
    echo "[10/11] Skipping smoke create job (RUN_SMOKE_CREATE=0)"
fi

# ── [11/11] Write environment files ──────────────────────────────────────────
echo "[11/11] Writing .env.local..."
cat > "$ROOT_DIR/.env.local" <<EOF
# Generated by scripts/deploy-local.sh
HTTP_RPC_ENDPOINT=$RPC_URL
WS_RPC_ENDPOINT=$WS_RPC_URL
PUBLIC_HTTP_RPC_ENDPOINT=$PUBLIC_HTTP_RPC_URL
PUBLIC_WS_RPC_ENDPOINT=$PUBLIC_WS_RPC_URL
CHAIN_ID=$ANVIL_CHAIN_ID

TANGLE_CONTRACT=$TANGLE
RESTAKING_CONTRACT=$RESTAKING
STATUS_REGISTRY_CONTRACT=$STATUS_REGISTRY

INSTANCE_BLUEPRINT_ID=$INSTANCE_BLUEPRINT_ID
TEE_INSTANCE_BLUEPRINT_ID=$TEE_INSTANCE_BLUEPRINT_ID

INSTANCE_SERVICE_ID=$INSTANCE_SERVICE_ID
TEE_INSTANCE_SERVICE_ID=$TEE_INSTANCE_SERVICE_ID

INSTANCE_OPERATOR_API_URL=http://$PUBLIC_HOST:$OPERATOR_API_PORT
TEE_OPERATOR_API_URL=http://$PUBLIC_HOST:$TEE_OPERATOR_API_PORT
OPENCLAW_OPERATOR_API_TOKEN=$OPENCLAW_OPERATOR_API_TOKEN
OPENCLAW_UI_ACCESS_TOKEN=$OPENCLAW_UI_ACCESS_TOKEN
OPENCLAW_ALLOW_WALLET_SIGNATURE_ACCESS_TOKEN_FALLBACK=$OPENCLAW_ALLOW_WALLET_SIGNATURE_ACCESS_TOKEN_FALLBACK
OPENCLAW_RUNTIME_BACKEND=$OPENCLAW_RUNTIME_BACKEND
OPENCLAW_IMAGE_OPENCLAW=$OPENCLAW_IMAGE_OPENCLAW
OPENCLAW_IMAGE_IRONCLAW=$OPENCLAW_IMAGE_IRONCLAW
OPENCLAW_IMAGE_NANOCLAW=$OPENCLAW_IMAGE_NANOCLAW
OPENCLAW_NANOCLAW_BUILD_CONTEXT=$OPENCLAW_NANOCLAW_BUILD_CONTEXT
SERVICE_CALLERS_ARRAY=$SERVICE_CALLERS_ARRAY

DEPLOYER_KEY=$DEPLOYER_KEY
DEPLOYER_ADDR=$DEPLOYER_ADDR
OPERATOR1_KEY=$OPERATOR1_KEY
OPERATOR1_ADDR=$OPERATOR1_ADDR
OPERATOR2_KEY=$OPERATOR2_KEY
OPERATOR2_ADDR=$OPERATOR2_ADDR
USER_KEY=$USER_KEY
USER_ADDR=$USER_ADDR
EOF

cat > "$ROOT_DIR/ui/.env.local" <<EOF
VITE_DEMO_MODE=0
VITE_CHAIN_ID=$ANVIL_CHAIN_ID
VITE_CHAIN_NAME=Tangle Local
VITE_CHAIN_CURRENCY_SYMBOL=ETH
VITE_RPC_URL=$PUBLIC_HTTP_RPC_URL
VITE_WS_RPC_URL=$PUBLIC_WS_RPC_URL
VITE_TANGLE_CONTRACT=$TANGLE
VITE_JOBS_ADDRESS=$TANGLE
VITE_SERVICES_ADDRESS=$TANGLE
VITE_INSTANCE_SERVICE_ID=$INSTANCE_SERVICE_ID
VITE_TEE_INSTANCE_SERVICE_ID=$TEE_INSTANCE_SERVICE_ID
VITE_OPERATOR_API_TOKEN=$OPENCLAW_OPERATOR_API_TOKEN
VITE_UI_ACCESS_TOKEN=$OPENCLAW_UI_ACCESS_TOKEN
EOF

echo ""
echo "========================================================================="
echo "  OpenClaw Local Testnet Ready"
echo "========================================================================="
echo "  Instance blueprint:   $INSTANCE_BLUEPRINT_ID"
echo "  TEE blueprint:        $TEE_INSTANCE_BLUEPRINT_ID"
echo "  Instance service:     $INSTANCE_SERVICE_ID"
echo "  TEE service:          $TEE_INSTANCE_SERVICE_ID"
echo ""
echo "  Operator APIs:"
echo "    Instance: http://$PUBLIC_HOST:$OPERATOR_API_PORT"
echo "    TEE:      http://$PUBLIC_HOST:$TEE_OPERATOR_API_PORT"
echo "  Browser RPC:"
echo "    HTTP:     $PUBLIC_HTTP_RPC_URL"
echo "    WS:       $PUBLIC_WS_RPC_URL"
echo "  Provision callers:"
echo "    $SERVICE_CALLERS_ARRAY"
echo ""
echo "  Auth tokens:"
echo "    Operator token: $OPENCLAW_OPERATOR_API_TOKEN"
echo "    Access token:   $OPENCLAW_UI_ACCESS_TOKEN"
echo ""
echo "  Example (list instances):"
echo "    curl -H 'Authorization: Bearer $OPENCLAW_OPERATOR_API_TOKEN' \\"
echo "      http://$PUBLIC_HOST:$OPERATOR_API_PORT/instances"
echo ""
echo "  Press Ctrl+C to stop operators + anvil"
echo "========================================================================="

wait

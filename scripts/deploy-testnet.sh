#!/usr/bin/env bash
# PrizeVault testnet bootstrap. Written against stellar-cli 28.0.0.
#
# NOT EXECUTED in the environment this project was built in: the build sandbox
# could not reach soroban-testnet.stellar.org, horizon-testnet.stellar.org or
# friendbot.stellar.org (see TOOLCHAIN.md at the repository root). Run it from a
# machine with testnet access; every step is idempotent enough to re-run.
#
# What it does:
#   1. generates and funds four keys: organizer, sponsor, referee, issuer (+ demo players)
#   2. builds and deploys the contract
#   3. wraps a fresh test asset (TUSD, issued by the issuer key) as a Stellar Asset
#      Contract and mints the pool to the sponsor, so the demo does not depend on a
#      third-party USDC faucet; set TOKEN_CONTRACT_ID to skip this and use real testnet USDC
#   4. creates the seed tournament (data/seed/tournament.json values), funds it in two
#      tranches and opens registration - the on-chain proof of funds
#   5. registers the demo player keys and prints the env lines for the app and web page
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

NETWORK="${STELLAR_NETWORK:-testnet}"
RPC_URL="${STELLAR_RPC_URL:-https://soroban-testnet.stellar.org}"
PREFIX="${PRIZEVAULT_KEY_PREFIX:-pv}"
WITH_REFEREE="${PRIZEVAULT_WITH_REFEREE:-1}"
DEMO_PLAYERS="${PRIZEVAULT_DEMO_PLAYERS:-2}"
WASM="target/wasm32v1-none/release/prizevault.wasm"

# Seed values (data/seed/tournament.json): 7,777.7777777 TUSD in two tranches.
POOL_STROOPS=77777777777
TRANCHE_1=50000000000
TRANCHE_2=$((POOL_STROOPS - TRANCHE_1))
PLACEMENTS='[3000,1800,1200,900,600,600,400,400,200,200,200,200,75,75,75,75]'
NOW="$(date +%s)"
REGISTER_BY=$((NOW + 7 * 86400))
RESULTS_BY=$((NOW + 14 * 86400))
CLAIM_BY=$((NOW + 44 * 86400))

need() { command -v "$1" >/dev/null 2>&1 || { echo "missing: $1" >&2; exit 1; }; }
need stellar
need cargo

ensure_key() {
  # stellar keys generate refuses to overwrite; reuse an existing key of the same name
  local name="$1"
  if stellar keys address "$name" >/dev/null 2>&1; then
    echo "key $name exists: $(stellar keys address "$name")"
  else
    stellar keys generate --network "$NETWORK" --fund "$name"
    echo "key $name created and funded: $(stellar keys address "$name")"
  fi
}

echo "== 1. keys"
ensure_key "$PREFIX-organizer"
ensure_key "$PREFIX-sponsor"
ensure_key "$PREFIX-issuer"
ORGANIZER_PK="$(stellar keys address "$PREFIX-organizer")"
SPONSOR_PK="$(stellar keys address "$PREFIX-sponsor")"
ISSUER_PK="$(stellar keys address "$PREFIX-issuer")"
REFEREE_PK=""
if [ "$WITH_REFEREE" = "1" ]; then
  ensure_key "$PREFIX-referee"
  REFEREE_PK="$(stellar keys address "$PREFIX-referee")"
fi

echo "== 2. build + deploy"
stellar contract build
CONTRACT_ID="$(stellar contract deploy \
  --wasm "$WASM" \
  --source-account "$PREFIX-organizer" \
  --network "$NETWORK" \
  --alias prizevault)"
echo "contract: $CONTRACT_ID"

echo "== 3. token"
if [ -n "${TOKEN_CONTRACT_ID:-}" ]; then
  TOKEN_ID="$TOKEN_CONTRACT_ID"
  echo "using existing token $TOKEN_ID (the sponsor must already hold >= $POOL_STROOPS base units)"
else
  ASSET="TUSD:$ISSUER_PK"
  TOKEN_ID="$(stellar contract asset deploy --asset "$ASSET" --source-account "$PREFIX-issuer" --network "$NETWORK")"
  echo "wrapped $ASSET as $TOKEN_ID"
  # classic accounts need a trustline before the SAC can credit them
  stellar tx new change-trust --source-account "$PREFIX-sponsor" --network "$NETWORK" --line "$ASSET"
  stellar contract invoke --id "$TOKEN_ID" --source-account "$PREFIX-issuer" --network "$NETWORK" -- \
    mint --to "$SPONSOR_PK" --amount "$POOL_STROOPS"
  echo "minted $POOL_STROOPS base units of TUSD to the sponsor"
fi

echo "== 4. create, fund, open (proof of funds)"
CREATE_ARGS=(
  --organizer "$ORGANIZER_PK"
  --sponsor "$SPONSOR_PK"
  --token "$TOKEN_ID"
  --pool_required "$POOL_STROOPS"
  --placements "$PLACEMENTS"
  --register_by "$REGISTER_BY"
  --results_by "$RESULTS_BY"
  --claim_by "$CLAIM_BY"
)
if [ -n "$REFEREE_PK" ]; then
  CREATE_ARGS+=(--referee "$REFEREE_PK")
fi
TOURNAMENT_ID="$(stellar contract invoke --id "$CONTRACT_ID" --source-account "$PREFIX-organizer" --network "$NETWORK" -- \
  create "${CREATE_ARGS[@]}")"
echo "tournament id: $TOURNAMENT_ID"

stellar contract invoke --id "$CONTRACT_ID" --source-account "$PREFIX-sponsor" --network "$NETWORK" -- \
  fund --id "$TOURNAMENT_ID" --from "$SPONSOR_PK" --amount "$TRANCHE_1"
stellar contract invoke --id "$CONTRACT_ID" --source-account "$PREFIX-sponsor" --network "$NETWORK" -- \
  fund --id "$TOURNAMENT_ID" --from "$SPONSOR_PK" --amount "$TRANCHE_2"
stellar contract invoke --id "$CONTRACT_ID" --source-account "$PREFIX-organizer" --network "$NETWORK" -- \
  open_registration --id "$TOURNAMENT_ID"
echo "registration open: pool of $POOL_STROOPS base units escrowed"

echo "== 5. demo players"
for i in $(seq 1 "$DEMO_PLAYERS"); do
  ensure_key "$PREFIX-player$i"
  PLAYER_PK="$(stellar keys address "$PREFIX-player$i")"
  # a claimant needs a trustline to receive the classic asset later
  if [ -z "${TOKEN_CONTRACT_ID:-}" ]; then
    stellar tx new change-trust --source-account "$PREFIX-player$i" --network "$NETWORK" --line "TUSD:$ISSUER_PK"
  fi
  stellar contract invoke --id "$CONTRACT_ID" --source-account "$PREFIX-player$i" --network "$NETWORK" -- \
    register --id "$TOURNAMENT_ID" --player "$PLAYER_PK"
  echo "registered player$i $PLAYER_PK"
done

echo "== status"
stellar contract invoke --id "$CONTRACT_ID" --source-account "$PREFIX-organizer" --network "$NETWORK" -- \
  status --id "$TOURNAMENT_ID"

ENV_FILE="app/.env.testnet"
cat > "$ENV_FILE" <<EOF
STELLAR_RPC_URL=$RPC_URL
STELLAR_NETWORK_PASSPHRASE=Test SDF Network ; September 2015
PRIZEVAULT_CONTRACT_ID=$CONTRACT_ID
TOKEN_CONTRACT_ID=$TOKEN_ID
TOKEN_DECIMALS=7
TOKEN_SYMBOL=TUSD
# organizer secret; swap for the sponsor / referee / player secret per command:
STELLAR_SECRET_KEY=$(stellar keys secret "$PREFIX-organizer")
EOF
echo
echo "wrote $ENV_FILE (git-ignored). Next:"
echo "  node --env-file=$ENV_FILE app/src/cli.ts proof-of-funds --id $TOURNAMENT_ID"
echo "  web/index.html?contract=$CONTRACT_ID&id=$TOURNAMENT_ID&rpc=$RPC_URL&symbol=TUSD"
echo "  after the event: prizevault results --id $TOURNAMENT_ID --file data/seed/results-fixed.json --roster data/seed/roster.json"
echo "  (the seed roster addresses are synthetic; on testnet, register real player keys first)"

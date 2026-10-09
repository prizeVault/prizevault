# PrizeVault

Esports prize pools escrowed on Stellar **before registration opens**, paid out by a fixed placement table once results are confirmed, with unclaimed prizes returned to the sponsor.

A Soroban contract holds the pool; a public proof-of-funds page lets any player, team or sponsor verify the money is there; the organizer (optionally co-signed by a referee) releases payouts by submitting a ranking; every winner claims their own share; whatever is not claimed by the deadline goes back to the sponsor.

## Problem

Esports prize money is paid 30–180+ days late, or never, because pools are announced before sponsor money is collected and payouts run through manual KYC and bank wires. Documented cases (all Researched, see `research/04-problems-consumer-creator-services.md` §P8 and `research/08-final-selection.md` §01):

- Esports World Cup 2024: allegations of prize money, salaries and vendor invoices unpaid about six months after the event.
- YaLLa Esports bankruptcy: more than $700k of prize money left unpaid.
- FISSURE acknowledged 15 teams as creditors (Astralis, FURIA, G2, Team Liquid among them) in June 2026 while planning a new $1M event.
- Fog Esports: 2+ months of salary arrears; player attrition and sponsor distrust follow.
- Payout vendors themselves market against "30-day waits".

The long tail is tier-2/3 organisers running Discord, Battlefy and Toornament events with $2k–$200k pools. None of the existing tools (i-payout, Tipalti, Community Gaming on Polygon) escrows the money *before* the event or gives a public proof of funds, and the Polygon route requires EVM wallets from non-crypto teams.

## User

- **User:** operations lead at a tier-2/3 tournament organiser (TO).
- **Problem owner:** TO founder. **Buyer:** the TO (SaaS or take-rate) or a sponsor/publisher that makes proof of funds a condition of sponsorship.
- **Economic beneficiaries:** teams and players (cash on time), sponsors (trust).

## What the MVP does

1. **Proof of funds.** `create` records the pool, the placement table (basis points summing to 10 000) and three deadlines. `fund` moves the token into the contract, capped at the announced pool. `open_registration` is refused until `pool_funded == pool_required`; that transition is the proof, and anyone can read it through `status` or the web page.
2. **Registration.** Players register themselves (their own signature) until `register_by`. Up to 1 024 players per tournament.
3. **Results.** After registration closes the organizer submits a ranking (first place first, at most as long as the placement table, every address registered, no duplicates). Without a referee the ranking is confirmed on the spot; with one, the referee confirms or rejects (and the organizer resubmits) before `results_by`. If `results_by` passes without confirmed results, anyone can `cancel_expired` and the sponsor is refunded.
4. **Payouts.** On confirmation each placement's share is computed with integer bps math (dust goes to the last placement, the amounts sum to the pool exactly). Winners `claim` until `claim_by`; each claim is a token transfer to the winner's own address.
5. **Sweep.** After `claim_by`, anyone can `refund_unclaimed`: the remaining balance (unclaimed prizes, unfilled placements) goes back to the sponsor and the tournament closes. The organizer can `cancel` at any point before results are confirmed; the sponsor gets everything back.

Everything is one contract instance hosting many tournaments; funds are tracked per tournament.

Also included: a TypeScript CLI (`prizevault`) with all of the above plus `proof-of-funds`, a static `web/index.html` that renders the proof-of-funds badge and placement table by simulating `status` through any Soroban RPC, and realistic seed data (64-player roster with messy gamertags and addresses, a 16-place table, a results export with a duplicate row that must be rejected).

## Quickstart (local, no network)

Prerequisites: Rust 1.94 with the `wasm32v1-none` target, stellar-cli 28.0.0, Node 22.18+ (see `TOOLCHAIN.md` at the repository root).

```sh
cd stellar/prizevault

# contract: 46 tests executed in the Soroban host (real auth, storage and token semantics)
cargo test
cargo test -- --nocapture scenario_64_player_bracket   # prints the 64-player walkthrough

# wasm (20 384 bytes, 19 exported functions)
stellar contract build

# app: 32 offline tests (placement math, results parsing, arg parsing, tx building, proof view)
cd app && npm install && npm test

# validate a bracket export without touching the network
node src/cli.ts results --id 0 --file ../data/seed/results.json --roster ../data/seed/roster.json --dry-run
```

`scripts/build.sh` runs all of the above in order and rebuilds `app/src/spec.json` (the contract spec extracted from the wasm, so the CLI needs no RPC round-trip to construct its client).

## Commands

Configuration comes from the environment (`.env.example` lists every variable; load it with `node --env-file=.env app/src/cli.ts ...`) or from `--rpc-url`, `--contract-id`, `--network-passphrase`, `--secret-key`.

| Command | Signer | What it does |
|---|---|---|
| `create --sponsor G.. --token C.. --pool 7777.7777777 --placements data/seed/placements.json --register-by +7d --results-by +14d --claim-by +44d [--referee G..]` | organizer | creates the tournament, prints its id |
| `fund --id 0 --amount 5000` | sponsor (or any funder) | escrows tokens; rejects amounts above the announced pool |
| `open --id 0` | organizer | opens registration; fails with `Underfunded` unless the pool is complete |
| `register --id 0` | player | joins the roster before `register_by` |
| `results --id 0 --file results.json [--roster roster.json] [--dry-run]` | organizer | validates the export (duplicates, unregistered, gaps, length) then submits the ranking |
| `confirm --id 0` / `reject --id 0` | referee | confirms (prizes become claimable) or rejects (organizer resubmits) |
| `claim --id 0` | winner | transfers the winner's share to their address |
| `refund --id 0` | anyone | after `claim_by`, sweeps unclaimed prizes to the sponsor, closes |
| `cancel --id 0 [--expired]` | organizer / anyone after `results_by` | refunds the sponsor in full |
| `status --id 0 [--json]` | none | prints the tournament record |
| `proof-of-funds --id 0 [--json]` | none | shareable summary: escrowed/required, state, deadlines, placement table |

Amounts are given in token units (`7777.7777777`, 7 decimals for USDC); deadlines accept ISO-8601, unix seconds or `+7d`/`+36h`/`+90m`.

## Proof-of-funds page

Open `web/index.html` (any static host or `file://`), enter the contract id, tournament id and RPC URL, or share a link such as `index.html?contract=C...&id=0&rpc=https://soroban-testnet.stellar.org&symbol=USDC`. The page loads `@stellar/stellar-sdk` from a CDN, builds a `status(id)` invocation from a null account, runs `simulateTransaction`, and renders the badge (FUNDED / PARTIALLY FUNDED / UNFUNDED / CANCELLED / CLOSED), the escrowed-vs-required bar, deadlines and the per-place prize table using the same integer math as the contract. No wallet, no transactions.

## Testnet

`scripts/deploy-testnet.sh` generates and funds keys, builds and deploys the contract, wraps a fresh test asset as a Stellar Asset Contract, mints the seed pool to the sponsor, creates the seed tournament, funds it in two tranches, opens registration and registers demo players. It was **written but not executed** in the build environment: testnet RPC, Horizon and Friendbot were unreachable there (`TOOLCHAIN.md`). See `DEMO.md` for the sequence to run on a machine with network access.

## Layout

```
contracts/prizevault/      Soroban contract (src/lib.rs) and tests (src/test.rs), test_snapshots/
app/                       TypeScript CLI, offline tests, spec extractor (scripts/extract-spec.ts)
web/index.html             static proof-of-funds page
data/seed/                 roster.json (64 players), placements.json, placements-invalid.json,
                           results.json (duplicate row), results-fixed.json, tournament.json
scripts/                   build.sh, deploy-testnet.sh (not executed here)
ARCHITECTURE.md            state machine, storage, auth, TTL, limits, why Stellar
VALIDATION.md              evidence tiers, baselines, experiment plan, killer questions
DEMO.md                    exact commands and expected output
```

## Limits and non-goals

- No admin key, no upgrade path, no fee: the contract is immutable per deployment and takes no cut (the commercial model is discussed in `VALIDATION.md`).
- Placement tables up to 256 places, rosters up to 1 024 players, pools up to `i128::MAX / 10 000` base units.
- Players claim to the address they registered with; wallet-less claim codes and claimable balances are future work.
- Results come from the organizer (plus an optional referee); there is no game-API oracle.

## Status

**functional locally** (contract verified by `cargo test` in the Soroban host, wasm built, CLI and page tested offline); deployment scripts are **testnet-ready** but were not executed here. No external users, no deployment, no measured metrics yet.

<!-- Managed by PrizeVault Contributors -->

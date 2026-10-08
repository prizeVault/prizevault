# PrizeVault demo

Two paths: the local one runs entirely inside the Soroban host and Node (no network) and is what was executed to verify this project; the testnet one is scripted but was **not executed** in the build environment (no testnet access there).

## 1. Local: the 64-player bracket in the Soroban host

```sh
cd stellar/prizevault
cargo test -- --nocapture scenario_64_player_bracket
```

The scenario creates a tournament with a referee and a 7 777.7777777 USDC pool over the 16-place seed table, funds it in two tranches (the first `open_registration` attempt fails with `Underfunded`), registers 64 players and refuses a 65th after the deadline, rejects a ranking with a duplicate entry, submits the corrected ranking, has the referee confirm it, lets 11 of 16 winners claim, refuses a double claim, a non-winner and an early sweep, then closes the claim window and refunds exactly the unclaimed remainder to the sponsor. It prints:

```
running 1 test
== PrizeVault scenario: 64-player bracket, 16 placements ==
pool_required = 77777777777 stroops (7,777.7777777 USDC)
registration opened: pool_funded = 77777777777 / 77777777777
roster closed with 64 players
duplicate ranking rejected
  place  1:  3000 bps ->    23333333333 stroops
  place  2:  1800 bps ->    13999999999 stroops
  place  3:  1200 bps ->     9333333333 stroops
  place  4:   900 bps ->     6999999999 stroops
  place  5:   600 bps ->     4666666666 stroops
  place  6:   600 bps ->     4666666666 stroops
  place  7:   400 bps ->     3111111111 stroops
  place  8:   400 bps ->     3111111111 stroops
  place  9:   200 bps ->     1555555555 stroops
  place 10:   200 bps ->     1555555555 stroops
  place 11:   200 bps ->     1555555555 stroops
  place 12:   200 bps ->     1555555555 stroops
  place 13:    75 bps ->      583333333 stroops
  place 14:    75 bps ->      583333333 stroops
  place 15:    75 bps ->      583333333 stroops
  place 16:    75 bps ->      583333340 stroops
11 winners claimed 59888888891 stroops
unclaimed 17888888886 stroops refunded to sponsor; vault balance 0; state Closed
Writing test snapshot file for test "test::scenario_64_player_bracket" to "test_snapshots/test/scenario_64_player_bracket.1.json".
test test::scenario_64_player_bracket ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 45 filtered out; finished in 0.42s
```

Points to notice: the 16 amounts sum to the pool exactly and the 7 stroops of rounding dust sit on the last place; 59 888 888 891 + 17 888 888 886 = 77 777 777 777; the token balance of the contract is zero at the end.

The whole suite:

```sh
cargo test
# test result: ok. 46 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

Among the 46: `open_registration_fails_when_underfunded`, `create_rejects_placements_not_summing_to_10000`, `referee_reject_then_resubmit_then_confirm`, `cancel_expired_only_after_results_by_and_needs_no_auth`, and a negative auth test for every signed function (`*_requires_*_auth`).

## 2. Local: wasm and app

```sh
stellar contract build
# Wasm File: target/wasm32v1-none/release/prizevault.wasm (20384 bytes optimized (original size was 24514 bytes))
# Exported Functions: 19 found

cd app && npm install && npm test
# tests 32 / pass 32 / fail 0
```

Validate the seed bracket export exactly as the organizer would before submitting, no network involved. The raw export has the champion pasted again on row 12:

```sh
node src/cli.ts results --id 0 --file ../data/seed/results.json --roster ../data/seed/roster.json --dry-run
```

```
error: ResultsError:
  - duplicate address GBF3R2AD6CVMLUM7WZ37BIPHEWU6RSHE5HZX3Y4LAU6I3ILMLCJ3H35G (xXShadowStrikeXx) at places 1 and 12: the contract rejects this ranking (DuplicateInRanking)
```

(exit code 1). The corrected export passes, and the roster's messiness is surfaced as warnings:

```sh
node src/cli.ts results --id 0 --file ../data/seed/results-fixed.json --roster ../data/seed/roster.json --dry-run
```

```
warning: roster row 5 gamertag: surrounding whitespace removed
warning: roster row 9 gamertag: surrounding whitespace removed
warning: roster row 18 address: surrounding whitespace removed
warning: roster row 63: gamertag "ghost" also used by seat 39 (different address, kept both)
ranking is valid: 16 places
    1. GBF3R2AD6CVMLUM7WZ37BIPHEWU6RSHE5HZX3Y4LAU6I3ILMLCJ3H35G
    2. GDV7JC7OGVQEKVKY2N7PBFH6LRMC6MAGOHSKYXUKG5AIKPVHVJGLQ3IB
    ...
   16. GB4JZVATIVLYL354WN7RV3SIYSXXYZLFQYEKKQV377XJU3TGXLBQHCGZ
dry run: nothing submitted
```

`node src/cli.ts --help` lists every command; `npm run build` emits `dist/cli.js` (the `prizevault` bin).

## 3. Testnet path (scripted, not executed here)

Requires a machine that can reach `https://soroban-testnet.stellar.org` and Friendbot. stellar-cli 28.0.0.

```sh
cd stellar/prizevault
scripts/deploy-testnet.sh
```

The script: generates and funds `pv-organizer`, `pv-sponsor`, `pv-referee`, `pv-issuer` and two demo players; builds and deploys the contract (alias `prizevault`); wraps `TUSD:<issuer>` as a Stellar Asset Contract, adds trustlines and mints 77 777 777 777 base units to the sponsor; creates the seed tournament (`register_by +7d`, `results_by +14d`, `claim_by +44d`, referee set); funds it in two tranches; opens registration; registers the demo players; prints `status`; writes `app/.env.testnet`.

Then, from the app directory:

```sh
node --env-file=.env.testnet src/cli.ts proof-of-funds --id 0
#   PrizeVault tournament #0: FUNDED
#   escrowed      7777.7777777 TUSD of 7777.7777777 TUSD (100.00%)
#   state         Open - registration open
#   ...
#   placements    16 places, 10000 bps
#     1.  30.00%  2333.3333333 TUSD
#     ...
#    16.   0.75%  58.333334 TUSD
```

Share the page: `web/index.html?contract=<C...>&id=0&rpc=https://soroban-testnet.stellar.org&symbol=TUSD` (any static host; the page loads the SDK from a CDN and simulates `status`).

After the event, with `STELLAR_SECRET_KEY` set to the organizer's secret:

```sh
node --env-file=.env.testnet src/cli.ts results --id 0 --file results.json --roster roster.json
# referee (its own secret):
node --env-file=.env.testnet src/cli.ts confirm --id 0
# each winner (their own secret):
node --env-file=.env.testnet src/cli.ts claim --id 0
# anyone, after claim_by:
node --env-file=.env.testnet src/cli.ts refund --id 0
```

`register_by` must have passed before `results` is accepted (`RegistrationStillOpen` otherwise); for a fast demo create the tournament with `--register-by +10m --results-by +1h --claim-by +2h` instead of the seed deadlines. The seed roster addresses are synthetic and cannot sign, so on testnet real player keys register first and the results file lists those.

Expected on-chain outcome, mirroring section 1: every claim is a token transfer to the winner, `paid_out` grows with each claim, and `refund` returns `pool_funded - paid_out` to the sponsor and sets the state to `Closed`.
